/**
 * SoundShared Listener — WebSocket audio player
 *
 * Wire format (server -> browser):
 *   [seq_no: u32 LE][pcm: f32 LE, interleaved stereo, 48 kHz]
 *
 * Playback uses an AudioWorklet with a ring buffer and a real jitter buffer:
 *   - wait for ~200 ms before starting
 *   - rebuffer when the queue drops below ~30 ms
 *   - cap latency at ~1.5 s
 *   - fade in/out over ~8 ms to avoid clicks
 *
 * The worklet runs on the audio rendering thread and reports buffer/underrun
 * statistics back to the main thread. No per-callback allocation or slicing.
 */

(function () {
    'use strict';

    // =========================================================================
    // Configuration
    // =========================================================================
    const CONFIG = {
        sampleRate: 48000,
        channels: 2,
        // Playback starts once this much audio is queued.
        targetBufferMs: 200,
        // If the queue falls below this, pause and rebuffer.
        lowWaterMs: 30,
        // Hard latency cap; older audio is discarded above this.
        maxBufferMs: 1500,
        // Ring buffer capacity.
        capacityMs: 2000,
        // UI refresh interval for fallback stats (ms).
        uiUpdateInterval: 100,
        reconnect: {
            initialDelay: 1000,
            maxDelay: 30000,
            factor: 1.8,
            jitter: 0.3,
            maxAttempts: Infinity
        }
    };

    // =========================================================================
    // AudioWorklet processor
    // Injected as a Blob so the server does not need an extra static route.
    // =========================================================================
    const WORKLET_CODE = `
class SoundSharedStreamProcessor extends AudioWorkletProcessor {
    constructor(options) {
        super();
        const opts = (options && options.processorOptions) || {};
        this.channels = opts.channels || 2;
        this.sampleRate = (typeof sampleRate === 'number' && sampleRate > 0)
            ? sampleRate
            : (opts.sampleRate || 48000);

        this.capacityFrames = Math.ceil(this.sampleRate * (opts.capacitySeconds || 2));
        this.capacity = this.capacityFrames * this.channels;
        this.ring = new Float32Array(this.capacity);

        this.writePos = 0;
        this.readPos = 0;
        this.available = 0;

        this.started = false;
        this.fadeGain = 0;
        this.fadeFrames = Math.max(1, Math.floor(this.sampleRate * 0.008));
        this.fadeStep = 1 / this.fadeFrames;

        this.targetFrames = Math.floor(this.sampleRate * (opts.targetMs || 200) / 1000);
        this.lowWaterFrames = Math.floor(this.sampleRate * (opts.lowWaterMs || 30) / 1000);
        this.maxFrames = Math.floor(this.sampleRate * (opts.maxMs || 1500) / 1000);

        this.underruns = 0;
        this.overruns = 0;
        this.playedFrames = 0;
        this.receivedFrames = 0;
        this.lastReportPlayed = 0;
        this.lastReportReceived = 0;

        this.port.onmessage = (event) => this.onMessage(event.data);
    }

    onMessage(msg) {
        if (!msg) return;
        if (msg.type === 'push' && msg.samples instanceof Float32Array) {
            this.push(msg.samples);
        } else if (msg.type === 'reset') {
            this.reset();
        }
    }

    push(samples) {
        let frames = Math.floor(samples.length / this.channels);
        if (frames <= 0) return;

        this.receivedFrames += frames;
        let incoming = frames * this.channels;

        if (frames > this.capacityFrames) {
            const skipFrames = frames - this.capacityFrames;
            samples = samples.subarray(skipFrames * this.channels);
            incoming = this.capacityFrames * this.channels;
            this.writePos = 0;
            this.readPos = 0;
            this.available = 0;
            this.overruns++;
        }

        if (this.available + incoming > this.capacity) {
            const excessFrames = Math.ceil(
                (this.available + incoming - this.capacity) / this.channels
            );
            this.discard(excessFrames);
            this.overruns++;
        }

        if (this.available + incoming > this.maxFrames * this.channels) {
            const excessFrames = Math.ceil(
                (this.available + incoming - this.maxFrames * this.channels) / this.channels
            );
            this.discard(excessFrames);
            this.overruns++;
        }

        const first = Math.min(incoming, this.capacity - this.writePos);
        this.ring.set(samples.subarray(0, first), this.writePos);
        if (first < incoming) {
            this.ring.set(samples.subarray(first, incoming), 0);
        }
        this.writePos = (this.writePos + incoming) % this.capacity;
        this.available += incoming;

        // Report while the buffer is filling so the UI shows progress even
        // though the audio thread is not consuming frames yet.
        if (this.receivedFrames - this.lastReportReceived >= Math.floor(this.sampleRate / 10)) {
            this.lastReportReceived = this.receivedFrames;
            this.report();
        }
    }

    discard(frames) {
        let samples = Math.min(frames * this.channels, this.available);
        samples -= samples % this.channels;
        this.readPos = (this.readPos + samples) % this.capacity;
        this.available -= samples;
    }

    reset() {
        this.writePos = 0;
        this.readPos = 0;
        this.available = 0;
        this.started = false;
        this.fadeGain = 0;
        this.underruns = 0;
        this.overruns = 0;
        this.playedFrames = 0;
        this.receivedFrames = 0;
        this.lastReportPlayed = 0;
        this.lastReportReceived = 0;
        this.report();
    }

    report() {
        this.port.postMessage({
            type: 'stats',
            availableFrames: Math.floor(this.available / this.channels),
            playedFrames: this.playedFrames,
            receivedFrames: this.receivedFrames,
            underruns: this.underruns,
            overruns: this.overruns,
            started: this.started,
            sampleRate: this.sampleRate
        });
    }

    process(inputs, outputs) {
        const output = outputs[0];
        const left = output[0];
        const right = output.length > 1 ? output[1] : null;
        const frames = left.length;

        const targetSamples = this.targetFrames * this.channels;
        const lowSamples = this.lowWaterFrames * this.channels;

        if (!this.started && this.available >= targetSamples) {
            this.started = true;
            this.report();
        }

        for (let i = 0; i < frames; i++) {
            let l = 0;
            let r = 0;
            const consuming = this.started || this.fadeGain > 0;

            if (consuming && this.available >= this.channels) {
                l = this.ring[this.readPos];
                r = this.ring[this.readPos + 1];
                this.readPos = (this.readPos + this.channels) % this.capacity;
                this.available -= this.channels;
                this.playedFrames++;
            }

            if (this.started) {
                this.fadeGain = Math.min(1, this.fadeGain + this.fadeStep);
            } else if (this.fadeGain > 0) {
                this.fadeGain = Math.max(0, this.fadeGain - this.fadeStep);
            }

            left[i] = l * this.fadeGain;
            if (right) {
                right[i] = r * this.fadeGain;
            }
        }

        if (this.started && this.available < lowSamples) {
            this.started = false;
            this.underruns++;
            this.report();
        } else if (this.playedFrames - this.lastReportPlayed >= Math.floor(this.sampleRate / 10)) {
            this.lastReportPlayed = this.playedFrames;
            this.report();
        }

        return true;
    }
}

registerProcessor('soundshared-stream-processor', SoundSharedStreamProcessor);
`;

    // =========================================================================
    // Main-thread ring buffer (ScriptProcessor fallback only)
    // =========================================================================
    class RingBuffer {
        constructor(capacityFrames, channels, maxFrames) {
            this.channels = channels;
            this.capacityFrames = capacityFrames;
            this.capacity = capacityFrames * channels;
            this.maxFrames = maxFrames;
            this.buffer = new Float32Array(this.capacity);
            this.writePos = 0;
            this.readPos = 0;
            this.available = 0;
            this.overruns = 0;
        }

        push(samples) {
            let frames = Math.floor(samples.length / this.channels);
            if (frames <= 0) return;

            let incoming = frames * this.channels;

            if (frames > this.capacityFrames) {
                const skipFrames = frames - this.capacityFrames;
                samples = samples.subarray(skipFrames * this.channels);
                incoming = this.capacityFrames * this.channels;
                this.writePos = 0;
                this.readPos = 0;
                this.available = 0;
                this.overruns++;
            }

            if (this.available + incoming > this.capacity) {
                const excessFrames = Math.ceil(
                    (this.available + incoming - this.capacity) / this.channels
                );
                this.discard(excessFrames);
                this.overruns++;
            }

            if (this.available + incoming > this.maxFrames * this.channels) {
                const excessFrames = Math.ceil(
                    (this.available + incoming - this.maxFrames * this.channels) / this.channels
                );
                this.discard(excessFrames);
                this.overruns++;
            }

            const first = Math.min(incoming, this.capacity - this.writePos);
            this.buffer.set(samples.subarray(0, first), this.writePos);
            if (first < incoming) {
                this.buffer.set(samples.subarray(first, incoming), 0);
            }
            this.writePos = (this.writePos + incoming) % this.capacity;
            this.available += incoming;
        }

        discard(frames) {
            let samples = Math.min(frames * this.channels, this.available);
            samples -= samples % this.channels;
            this.readPos = (this.readPos + samples) % this.capacity;
            this.available -= samples;
        }

        reset() {
            this.writePos = 0;
            this.readPos = 0;
            this.available = 0;
            this.overruns = 0;
            this.buffer.fill(0);
        }
    }

    // =========================================================================
    // State
    // =========================================================================
    const state = {
        // Connection
        ws: null,
        wsUrl: '',
        reconnectAttempt: 0,
        reconnectTimer: null,
        isConnecting: false,
        shouldReconnect: false,

        // Audio graph
        audioContext: null,
        workletNode: null,
        analyserNode: null,
        gainNode: null,
        playbackMode: null, // 'worklet' | 'script' | null
        timeData: null,

        // Fallback ScriptProcessor
        scriptProcessor: null,
        scriptRing: null,
        scriptStarted: false,
        scriptFade: 0,

        // Buffer / playback statistics
        bufferFrames: 0,
        bufferSampleRate: CONFIG.sampleRate,
        workletStarted: false,
        packetsReceived: 0,
        framesReceived: 0,
        framesPlayed: 0,
        underruns: 0,
        overruns: 0,
        missedFrames: 0,

        // Sequence tracking
        lastSeqNo: null,
        lastPacketFrames: 0,
        lastPacketAt: 0,
        streamActive: false,

        // Meter peak hold
        peakLevel: 0,
        peakHoldUntil: 0,

        // UI loop
        animationFrame: null,
        lastUiUpdate: 0
    };

    // =========================================================================
    // DOM
    // =========================================================================
    const els = {};
    let bufferCells = [];

    function cacheElements() {
        els.statusIndicator = document.getElementById('statusIndicator');
        els.statusText = document.getElementById('statusText');

        els.connectBtn = document.getElementById('connectBtn');
        els.btnText = document.getElementById('btnText');
        els.btnIcon = document.getElementById('btnIcon');
        els.btnLoader = document.getElementById('btnLoader');

        els.audioIndicator = document.getElementById('audioIndicator');
        els.levelFill = document.getElementById('levelFill');
        els.levelPeak = document.getElementById('levelPeak');
        els.meterValue = document.getElementById('meterValue');

        els.bufferGrid = document.getElementById('bufferGrid');
        els.bufferText = document.getElementById('bufferText');
        bufferCells = els.bufferGrid ? Array.from(els.bufferGrid.children) : [];

        els.messagePanel = document.getElementById('messagePanel');
        els.messageIcon = document.getElementById('messageIcon');
        els.messageText = document.getElementById('messageText');
        els.dismissBtn = document.getElementById('dismissBtn');

        els.systemToast = document.getElementById('systemToast');
        els.systemToastIcon = document.getElementById('systemToastIcon');
        els.systemToastText = document.getElementById('systemToastText');

        els.wsUrl = document.getElementById('wsUrl');
        els.infoProtocol = document.getElementById('infoProtocol');
        els.infoBitrate = document.getElementById('infoBitrate');

        els.statsReceived = document.getElementById('statsReceived');
        els.statsPlayed = document.getElementById('statsPlayed');
        els.statsUnderruns = document.getElementById('statsUnderruns');
    }

    // =========================================================================
    // Logging / formatting
    // =========================================================================
    function log() {
        console.log('[SoundShared]', new Date().toISOString(), ...arguments);
    }

    function error() {
        console.error('[SoundShared]', new Date().toISOString(), ...arguments);
    }

    function formatNumber(value) {
        return Number(value || 0).toLocaleString('en-US');
    }

    // =========================================================================
    // UI state helpers
    // =========================================================================
    function setStatus(status, text) {
        if (els.statusIndicator) {
            els.statusIndicator.className = 'status-dot ' + status;
        }
        if (els.statusText) {
            els.statusText.className = 'status-text ' + status;
            els.statusText.textContent = text;
        }
    }

    function setButtonState(mode) {
        if (!els.connectBtn) return;

        if (mode === 'connecting') {
            els.connectBtn.disabled = true;
            els.btnText.textContent = 'Linking...';
            els.btnIcon.hidden = true;
            els.btnLoader.hidden = false;
            return;
        }

        els.connectBtn.disabled = false;
        els.btnLoader.hidden = true;
        els.btnIcon.hidden = false;

        if (mode === 'connected') {
            els.btnText.textContent = 'Terminate Link';
            els.btnIcon.setAttribute('icon', 'lucide:power');
        } else {
            els.btnText.textContent = 'Establish Link';
            els.btnIcon.setAttribute('icon', 'lucide:zap');
        }
    }

    function setSystemToast(text, active, icon) {
        if (!els.systemToast) return;
        if (els.systemToastText) els.systemToastText.textContent = text;
        els.systemToast.classList.toggle('is-active', !!active);
        if (icon && els.systemToastIcon) {
            els.systemToastIcon.setAttribute('icon', icon);
        }
    }

    function showMessage(message, severity, showDismiss) {
        if (!els.messagePanel) return;

        const kind = severity || 'error';
        const icons = {
            error: 'lucide:alert-triangle',
            info: 'lucide:info',
            success: 'lucide:check-circle'
        };

        els.messagePanel.className = 'toast toast--' + kind;
        els.messagePanel.hidden = false;
        els.messageText.textContent = message;
        els.messageIcon.setAttribute('icon', icons[kind] || icons.info);
        els.dismissBtn.hidden = showDismiss === false;
    }

    function hideMessage() {
        if (els.messagePanel) els.messagePanel.hidden = true;
    }

    function updateWsUrlUI() {
        if (!els.wsUrl) return;
        const url = state.wsUrl || buildWsUrl();
        els.wsUrl.textContent = url;
        els.wsUrl.title = url;
    }

    function updateInfoUI(format) {
        if (!els.infoProtocol || !els.infoBitrate) return;

        const sampleRate = (format && format.sample_rate) || CONFIG.sampleRate;
        const channels = (format && format.channels) || CONFIG.channels;
        const formatName = String((format && format.format) || 'f32').toUpperCase();

        els.infoProtocol.textContent =
            formatName + ' / ' + Math.round(sampleRate / 1000) + 'KHZ';
        els.infoBitrate.textContent =
            Math.round((sampleRate * channels * 32) / 1000) + ' KBPS';
    }

    function updateBufferUI() {
        if (!els.bufferText || !els.bufferGrid) return;

        const frames = Math.max(0, state.bufferFrames);
        const sampleRate = state.bufferSampleRate || CONFIG.sampleRate;
        const ms = (frames / sampleRate) * 1000;
        const target = CONFIG.targetBufferMs;
        const pct = Math.max(0, Math.min(1, ms / target));
        const activeCells = Math.round(pct * bufferCells.length);

        els.bufferText.textContent = Math.round(ms) + 'ms / ' + target + 'ms';

        for (let i = 0; i < bufferCells.length; i++) {
            const active = i < activeCells;
            bufferCells[i].classList.toggle('active', active);
            bufferCells[i].classList.toggle(
                'warn',
                active && ms > CONFIG.maxBufferMs * 0.75
            );
        }

        els.bufferGrid.classList.toggle(
            'is-low',
            state.streamActive && ms < CONFIG.lowWaterMs
        );
    }

    function updateStatsUI() {
        if (els.statsReceived) {
            els.statsReceived.textContent = formatNumber(state.packetsReceived);
        }
        if (els.statsPlayed) {
            els.statsPlayed.textContent = formatNumber(state.framesPlayed);
        }
        if (els.statsUnderruns) {
            els.statsUnderruns.textContent = formatNumber(state.underruns);
        }
    }

    function updateMeter() {
        if (!state.analyserNode || !state.timeData) return;

        state.analyserNode.getFloatTimeDomainData(state.timeData);

        let sum = 0;
        let peak = 0;
        for (let i = 0; i < state.timeData.length; i++) {
            const value = state.timeData[i];
            sum += value * value;
            const abs = Math.abs(value);
            if (abs > peak) peak = abs;
        }

        const rms = Math.sqrt(sum / state.timeData.length);
        const db = rms > 0 ? 20 * Math.log10(rms) : -Infinity;
        const level = db === -Infinity ? 0 : Math.max(0, Math.min(1, (db + 60) / 60));

        if (els.levelFill) {
            els.levelFill.style.width = (level * 100).toFixed(1) + '%';
        }
        if (els.meterValue) {
            els.meterValue.textContent = db === -Infinity ? '-INF dB' : db.toFixed(1) + ' dB';
        }
        if (els.audioIndicator) {
            els.audioIndicator.classList.toggle('is-clipping', level > 0.95);
        }

        const peakDb = peak > 0 ? 20 * Math.log10(peak) : -Infinity;
        const peakLevel =
            peakDb === -Infinity ? 0 : Math.max(0, Math.min(1, (peakDb + 60) / 60));

        // Peak-hold marker: jump to a new peak immediately, then hold it for a
        // moment before the slow decay in updateUI() takes over.
        const now = performance.now();
        if (peakLevel > state.peakLevel) {
            state.peakLevel = peakLevel;
            state.peakHoldUntil = now + 900;
        }

        if (els.levelPeak) {
            els.levelPeak.style.left =
                'calc(' + (state.peakLevel * 100).toFixed(1) + '% - 1px)';
        }
    }

    // =========================================================================
    // Audio graph
    // =========================================================================
    async function initAudio() {
        if (state.audioContext) {
            if (state.audioContext.state === 'suspended') {
                await state.audioContext.resume();
            }
            return;
        }

        const AudioContextCtor = window.AudioContext || window.webkitAudioContext;
        if (!AudioContextCtor) {
            throw new Error('Web Audio API is not supported in this browser');
        }

        let context;
        try {
            context = new AudioContextCtor({
                sampleRate: CONFIG.sampleRate,
                latencyHint: 'interactive'
            });
        } catch (e) {
            // Some browsers reject an explicit sample rate; use the default.
            context = new AudioContextCtor({ latencyHint: 'interactive' });
        }

        state.audioContext = context;
        if (context.sampleRate !== CONFIG.sampleRate) {
            log(
                'AudioContext sample rate is',
                context.sampleRate,
                'Hz; source stream is',
                CONFIG.sampleRate,
                'Hz'
            );
        }

        state.analyserNode = context.createAnalyser();
        state.analyserNode.fftSize = 1024;
        state.analyserNode.smoothingTimeConstant = 0.5;
        state.timeData = new Float32Array(state.analyserNode.fftSize);

        state.gainNode = context.createGain();
        state.gainNode.gain.value = 1.0;

        try {
            const blob = new Blob([WORKLET_CODE], { type: 'application/javascript' });
            const blobUrl = URL.createObjectURL(blob);
            await context.audioWorklet.addModule(blobUrl);
            URL.revokeObjectURL(blobUrl);

            const node = new AudioWorkletNode(context, 'soundshared-stream-processor', {
                numberOfInputs: 0,
                numberOfOutputs: 1,
                outputChannelCount: [CONFIG.channels],
                processorOptions: {
                    sampleRate: CONFIG.sampleRate,
                    channels: CONFIG.channels,
                    targetMs: CONFIG.targetBufferMs,
                    lowWaterMs: CONFIG.lowWaterMs,
                    maxMs: CONFIG.maxBufferMs,
                    capacitySeconds: CONFIG.capacityMs / 1000
                }
            });

            node.port.onmessage = onWorkletMessage;
            node.connect(state.analyserNode);
            state.analyserNode.connect(state.gainNode);
            state.gainNode.connect(context.destination);

            state.workletNode = node;
            state.playbackMode = 'worklet';
            log('AudioWorklet ready');
        } catch (workletError) {
            error('AudioWorklet unavailable; using ScriptProcessor fallback:', workletError);
            setupScriptProcessor();
        }

        if (context.state === 'suspended') {
            await context.resume();
        }
    }

    function setupScriptProcessor() {
        const context = state.audioContext;

        state.scriptRing = new RingBuffer(
            Math.ceil((CONFIG.sampleRate * CONFIG.capacityMs) / 1000),
            CONFIG.channels,
            Math.floor((CONFIG.sampleRate * CONFIG.maxBufferMs) / 1000)
        );

        const processor = context.createScriptProcessor(
            2048,
            CONFIG.channels,
            CONFIG.channels
        );
        processor.onaudioprocess = onScriptProcess;

        processor.connect(state.analyserNode);
        state.analyserNode.connect(state.gainNode);
        state.gainNode.connect(context.destination);

        state.scriptProcessor = processor;
        state.playbackMode = 'script';
        log('ScriptProcessorNode fallback ready');
    }

    function onWorkletMessage(event) {
        const msg = event.data;
        if (!msg || msg.type !== 'stats') return;

        state.bufferFrames = msg.availableFrames;
        if (msg.sampleRate) state.bufferSampleRate = msg.sampleRate;
        state.framesPlayed = msg.playedFrames;
        state.underruns = msg.underruns;
        state.overruns = msg.overruns;
        state.workletStarted = msg.started;

        updateBufferUI();
        updateStatsUI();
    }

    function onScriptProcess(event) {
        const ring = state.scriptRing;
        if (!ring) return;

        const outL = event.outputBuffer.getChannelData(0);
        const outR = event.outputBuffer.getChannelData(1);
        const frames = outL.length;

        const targetSamples =
            Math.floor((CONFIG.sampleRate * CONFIG.targetBufferMs) / 1000) * CONFIG.channels;
        const lowSamples =
            Math.floor((CONFIG.sampleRate * CONFIG.lowWaterMs) / 1000) * CONFIG.channels;
        const fadeStep = 1 / Math.max(1, Math.floor(CONFIG.sampleRate * 0.008));

        if (!state.scriptStarted && ring.available >= targetSamples) {
            state.scriptStarted = true;
        }

        for (let i = 0; i < frames; i++) {
            let l = 0;
            let r = 0;
            const consuming = state.scriptStarted || state.scriptFade > 0;

            if (consuming && ring.available >= CONFIG.channels) {
                l = ring.buffer[ring.readPos];
                r = ring.buffer[ring.readPos + 1];
                ring.readPos = (ring.readPos + CONFIG.channels) % ring.capacity;
                ring.available -= CONFIG.channels;
                state.framesPlayed++;
            }

            if (state.scriptStarted) {
                state.scriptFade = Math.min(1, state.scriptFade + fadeStep);
            } else if (state.scriptFade > 0) {
                state.scriptFade = Math.max(0, state.scriptFade - fadeStep);
            }

            outL[i] = l * state.scriptFade;
            outR[i] = r * state.scriptFade;
        }

        if (state.scriptStarted && ring.available < lowSamples) {
            state.scriptStarted = false;
            state.underruns++;
        }

        state.bufferFrames = Math.floor(ring.available / CONFIG.channels);
        state.overruns = ring.overruns;

        const now = performance.now();
        if (now - state.lastUiUpdate >= CONFIG.uiUpdateInterval) {
            state.lastUiUpdate = now;
            updateBufferUI();
            updateStatsUI();
        }
    }

    function pushSamples(samples) {
        if (!samples || samples.length < CONFIG.channels) return;

        if (state.playbackMode === 'worklet' && state.workletNode) {
            // Transfer ownership of the PCM buffer; no copy on the main thread.
            state.workletNode.port.postMessage(
                { type: 'push', samples: samples },
                [samples.buffer]
            );
        } else if (state.playbackMode === 'script' && state.scriptRing) {
            state.scriptRing.push(samples);
        }
    }

    // =========================================================================
    // Audio frame parsing
    // =========================================================================
    function handleBinary(arrayBuffer) {
        if (!arrayBuffer || arrayBuffer.byteLength < 8) return;

        const pcmBytes = arrayBuffer.byteLength - 4;
        if (pcmBytes % 4 !== 0) {
            log('Dropping malformed frame: PCM length is not 4-byte aligned', pcmBytes);
            return;
        }

        const sampleCount = pcmBytes / 4;
        if (sampleCount < CONFIG.channels || sampleCount % CONFIG.channels !== 0) {
            log('Dropping malformed frame: invalid sample count', sampleCount);
            return;
        }

        const view = new DataView(arrayBuffer);
        const seq = view.getUint32(0, true);
        const samples = new Float32Array(arrayBuffer, 4, sampleCount);
        const frames = sampleCount / CONFIG.channels;

        // Sequence gap detection. Insert silence for short gaps so the
        // playback timeline stays aligned; ignore huge jumps (reconnect).
        if (state.lastSeqNo !== null) {
            const gap = (seq - state.lastSeqNo) >>> 0;

            if (gap === 0) {
                return; // duplicate packet
            }

            if (gap > 1) {
                const missed = gap - 1;
                if (missed <= 50 && state.lastPacketFrames > 0) {
                    const silenceSamples =
                        missed * state.lastPacketFrames * CONFIG.channels;
                    pushSamples(new Float32Array(silenceSamples));
                    state.missedFrames += missed;
                } else {
                    log('Large sequence gap; skipping silence insertion:', missed);
                }
            }
        }

        state.lastSeqNo = seq;
        state.lastPacketFrames = frames;
        state.packetsReceived++;
        state.framesReceived += frames;
        state.lastPacketAt = performance.now();
        state.streamActive = true;

        pushSamples(samples);

        if (state.packetsReceived === 1) {
            setSystemToast('Stream active. Decoding 48 kHz PCM.', true, 'lucide:radio');
            hideMessage();
        }

        updateStatsUI();
    }

    function handleControlMessage(msg) {
        if (!msg || typeof msg.type !== 'string') return;

        switch (msg.type) {
            case 'format':
                updateInfoUI(msg.data || {});
                log('Server format:', msg.data);
                break;

            case 'welcome':
                log('Welcome from server:', msg.data);
                break;

            case 'error':
                showMessage(msg.message || 'Server error', 'error');
                break;

            case 'info':
                showMessage(msg.message || 'Info from server', 'info', false);
                break;

            default:
                log('Unknown control message:', msg.type);
        }
    }

    // =========================================================================
    // WebSocket
    // =========================================================================
    function buildWsUrl() {
        const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
        const host = location.host || 'localhost:8080';
        return protocol + '//' + host + '/ws/audio';
    }

    function connect() {
        if (
            state.ws &&
            (state.ws.readyState === WebSocket.OPEN ||
                state.ws.readyState === WebSocket.CONNECTING)
        ) {
            return;
        }

        state.wsUrl = buildWsUrl();
        updateWsUrlUI();
        state.shouldReconnect = true;
        state.reconnectAttempt = 0;
        doConnect();
    }

    function doConnect() {
        if (!state.shouldReconnect) return;

        state.isConnecting = true;
        setStatus('connecting', 'Linking...');
        setButtonState('connecting');
        setSystemToast('Negotiating WebSocket handshake...', false, 'lucide:radio-tower');
        hideMessage();

        log('Connecting to', state.wsUrl);

        try {
            const ws = new WebSocket(state.wsUrl);
            ws.binaryType = 'arraybuffer';
            ws.onopen = onWsOpen;
            ws.onmessage = onWsMessage;
            ws.onclose = onWsClose;
            ws.onerror = onWsError;
            state.ws = ws;
        } catch (e) {
            handleConnectionError(e);
        }
    }

    function onWsOpen() {
        log('WebSocket connected');
        state.isConnecting = false;
        state.reconnectAttempt = 0;

        setStatus('connected', 'Node Online');
        setButtonState('connected');
        setSystemToast('Link established. Waiting for audio frames.', true, 'lucide:radio');

        resetAudioPipeline();
    }

    function onWsMessage(event) {
        if (typeof event.data === 'string') {
            try {
                handleControlMessage(JSON.parse(event.data));
            } catch (e) {
                log('Text message from server:', event.data);
            }
            return;
        }

        if (event.data instanceof ArrayBuffer) {
            handleBinary(event.data);
        }
    }

    function onWsClose(event) {
        log('WebSocket closed:', event.code, event.reason);
        state.isConnecting = false;
        state.ws = null;
        state.streamActive = false;

        resetAudioPipeline();
        updateBufferUI();
        updateStatsUI();

        if (state.shouldReconnect) {
            if (event.code === 1000) {
                setStatus('disconnected', 'Node Offline');
                setButtonState('idle');
                setSystemToast('Link closed. Standing by.', false, 'lucide:check-circle');
                showMessage('Disconnected from server.', 'info', false);
            } else {
                setStatus('disconnected', 'Node Offline');
                setButtonState('idle');
                scheduleReconnect(event.reason || 'Code ' + event.code);
            }
        } else {
            setStatus('disconnected', 'Node Offline');
            setButtonState('idle');
            setSystemToast('System ready. Initializing components.', false, 'lucide:check-circle');
        }
    }

    function onWsError(event) {
        error('WebSocket error:', event);
        // `onclose` follows and drives the reconnect/state handling.
    }

    function handleConnectionError(err) {
        state.isConnecting = false;
        state.shouldReconnect = false;
        setStatus('error', 'Link Error');
        setButtonState('idle');
        setSystemToast('Connection failed. Check the server.', false, 'lucide:alert-triangle');
        showMessage(
            'Failed to connect: ' + (err && err.message ? err.message : err),
            'error'
        );
    }

    function scheduleReconnect(reason) {
        if (!state.shouldReconnect) return;

        state.reconnectAttempt++;
        const { initialDelay, maxDelay, factor, jitter, maxAttempts } = CONFIG.reconnect;

        if (state.reconnectAttempt > maxAttempts) {
            setStatus('error', 'Link Error');
            setSystemToast('Reconnection attempts exhausted.', false, 'lucide:alert-triangle');
            showMessage('Max reconnection attempts reached. Refresh the page.', 'error');
            return;
        }

        const base = Math.min(
            initialDelay * Math.pow(factor, state.reconnectAttempt - 1),
            maxDelay
        );
        const delay = Math.floor(base + base * jitter * Math.random());

        log(
            'Reconnect attempt',
            state.reconnectAttempt,
            'in',
            delay + 'ms',
            '(' + reason + ')'
        );

        showMessage(
            'Reconnecting in ' + (delay / 1000).toFixed(1) + 's...',
            'info',
            false
        );
        setSystemToast(
            'Reconnecting in ' + (delay / 1000).toFixed(1) + 's...',
            false,
            'lucide:radio-tower'
        );

        if (state.reconnectTimer) clearTimeout(state.reconnectTimer);
        state.reconnectTimer = setTimeout(function () {
            if (state.shouldReconnect) doConnect();
        }, delay);
    }

    function disconnect() {
        log('Manual disconnect');
        state.shouldReconnect = false;

        if (state.reconnectTimer) {
            clearTimeout(state.reconnectTimer);
            state.reconnectTimer = null;
        }

        if (state.ws) {
            try {
                state.ws.close(1000, 'User disconnected');
            } catch (e) {
                // Ignore close errors.
            }
            state.ws = null;
        }

        if (state.audioContext && state.audioContext.state === 'running') {
            state.audioContext.suspend().catch(function () {});
        }

        state.isConnecting = false;
        state.streamActive = false;
        resetAudioPipeline();
        updateBufferUI();
        updateStatsUI();

        setStatus('disconnected', 'Node Offline');
        setButtonState('idle');
        setSystemToast('System ready. Initializing components.', false, 'lucide:check-circle');
        hideMessage();
    }

    function resetAudioPipeline() {
        state.bufferFrames = 0;
        state.workletStarted = false;
        state.packetsReceived = 0;
        state.framesReceived = 0;
        state.framesPlayed = 0;
        state.underruns = 0;
        state.overruns = 0;
        state.missedFrames = 0;
        state.lastSeqNo = null;
        state.lastPacketFrames = 0;
        state.scriptStarted = false;
        state.scriptFade = 0;
        state.peakLevel = 0;
        state.peakHoldUntil = 0;

        if (state.workletNode) {
            state.workletNode.port.postMessage({ type: 'reset' });
        }
        if (state.scriptRing) {
            state.scriptRing.reset();
        }

        if (els.levelFill) els.levelFill.style.width = '0%';
        if (els.levelPeak) els.levelPeak.style.left = '0%';
        if (els.meterValue) els.meterValue.textContent = '-INF dB';
        if (els.audioIndicator) els.audioIndicator.classList.remove('is-clipping');
    }

    // =========================================================================
    // UI loop
    // =========================================================================
    function updateUI(now) {
        const t = typeof now === 'number' ? now : performance.now();

        updateMeter();

        if (state.streamActive && t - state.lastPacketAt > 1000) {
            state.streamActive = false;
            updateBufferUI();
        }

        if (t > state.peakHoldUntil && state.peakLevel > 0) {
            state.peakLevel = Math.max(0, state.peakLevel - 0.01);
            if (els.levelPeak) {
                els.levelPeak.style.left =
                    'calc(' + (state.peakLevel * 100).toFixed(1) + '% - 1px)';
            }
        }

        state.animationFrame = requestAnimationFrame(updateUI);
    }

    // =========================================================================
    // Events
    // =========================================================================
    async function onConnectClick() {
        if (state.isConnecting) return;

        if (state.ws && state.ws.readyState === WebSocket.OPEN) {
            disconnect();
            return;
        }

        try {
            await initAudio();

            if (state.audioContext.state === 'suspended') {
                await state.audioContext.resume();
            }

            connect();
        } catch (e) {
            error('Failed to initialize audio:', e);
            setStatus('error', 'Audio Error');
            setButtonState('idle');
            setSystemToast('Audio initialization failed.', false, 'lucide:alert-triangle');
            showMessage(
                'Failed to initialize audio: ' +
                    (e && e.message ? e.message : e) +
                    '. Check browser audio permissions.',
                'error'
            );
        }
    }

    function onVisibilityChange() {
        // Keep the stream running while hidden. The worklet continues to
        // consume audio; the ring buffer caps latency, so there is no need to
        // drop packets or clear the queue when the tab becomes visible again.
        if (
            !document.hidden &&
            state.audioContext &&
            state.audioContext.state === 'suspended' &&
            state.ws &&
            state.ws.readyState === WebSocket.OPEN
        ) {
            state.audioContext.resume().catch(function () {});
        }
    }

    function onBeforeUnload() {
        disconnect();
    }

    // =========================================================================
    // Init
    // =========================================================================
    function init() {
        cacheElements();

        updateWsUrlUI();
        updateInfoUI({
            sample_rate: CONFIG.sampleRate,
            channels: CONFIG.channels,
            format: 'f32'
        });

        setStatus('disconnected', 'Node Offline');
        setButtonState('idle');
        updateBufferUI();
        updateStatsUI();

        els.connectBtn.addEventListener('click', onConnectClick);
        els.dismissBtn.addEventListener('click', hideMessage);
        document.addEventListener('visibilitychange', onVisibilityChange);
        window.addEventListener('beforeunload', onBeforeUnload);

        state.animationFrame = requestAnimationFrame(updateUI);

        log('SoundShared Listener initialized');
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }

    // Expose for debugging.
    window.SoundShared = {
        getState: function () {
            return Object.assign({}, state);
        },
        connect: connect,
        disconnect: disconnect
    };
})();
