/**
 * SoundShared Listener - WebSocket Audio Player
 * Handles WebSocket connection, Web Audio API playback via AudioWorkletNode,
 * and UI state management. Rewritten to fix audio stuttering caused by
 * ScriptProcessorNode running on the main thread.
 */

(function() {
    'use strict';

    // =============================================================================
    // Configuration
    // =============================================================================
    const CONFIG = {
        sampleRate: 48000,
        channels: 2,
        // Target playback buffer before starting (ms) - higher = more resilient to jitter
        targetBufferMs: 400,
        // Maximum buffer duration before dropping old data (ms)
        maxBufferMs: 2000,
        // Warn level (ms) - below this shows buffer low indicator
        lowBufferMs: 150,
        // Reconnection settings
        reconnect: {
            initialDelay: 1000,
            maxDelay: 30000,
            factor: 1.8,
            jitter: 0.3,
            maxAttempts: Infinity
        },
        // UI update interval (ms)
        uiUpdateInterval: 100
    };

    // =============================================================================
    // AudioWorklet Processor Code (runs on the audio rendering thread)
    // Injected as Blob to avoid needing a separate file served by the server.
    // =============================================================================
    const WORKLET_CODE = `
class AudioStreamProcessor extends AudioWorkletProcessor {
    constructor() {
        super();
        // Interleaved stereo Float32 buffer: [L, R, L, R, ...]
        this.buffer = new Float32Array(0);
        this.port.onmessage = (event) => {
            if (event.data && event.data.channelData instanceof Float32Array) {
                const newData = event.data.channelData;
                const combined = new Float32Array(this.buffer.length + newData.length);
                combined.set(this.buffer);
                combined.set(newData, this.buffer.length);
                this.buffer = combined;
            }
        };
    }

    process(inputs, outputs, parameters) {
        const output = outputs[0];
        const outputL = output.getChannelData(0);
        const outputR = output.getChannelData(1);
        const framesToFill = outputL.length;
        const samplesToFill = framesToFill * 2;

        let samplesFilled = 0;

        // Consume from our internal buffer
        while (samplesFilled < samplesToFill && this.buffer.length >= 2) {
            const samplesToCopy = Math.min(samplesToFill - samplesFilled, this.buffer.length);
            const framesToCopy = Math.floor(samplesToCopy / 2);
            const samplesToCopyActual = framesToCopy * 2;

            for (let i = 0; i < framesToCopy; i++) {
                const outIdx = samplesFilled / 2 + i;
                outputL[outIdx] = this.buffer[samplesFilled + i * 2];
                outputR[outIdx] = this.buffer[samplesFilled + i * 2 + 1];
            }

            samplesFilled += samplesToCopyActual;

            // Drop consumed samples from the front
            if (samplesToCopyActual < this.buffer.length) {
                this.buffer = this.buffer.slice(samplesToCopyActual);
            } else {
                this.buffer = new Float32Array(0);
            }
        }

        // Fill any remaining frames with silence (underrun protection)
        const framesRemaining = framesToFill - samplesFilled / 2;
        for (let i = 0; i < framesRemaining; i++) {
            const idx = samplesFilled / 2 + i;
            outputL[idx] = 0;
            outputR[idx] = 0;
        }

        return true;
    }
}

registerProcessor('audio-stream-processor', AudioStreamProcessor);
`;

    // =============================================================================
    // State
    // =============================================================================
    const state = {
        // Connection
        ws: null,
        wsUrl: '',
        reconnectAttempt: 0,
        reconnectTimer: null,
        isConnecting: false,
        shouldReconnect: false,

        // Audio
        audioContext: null,
        audioWorkletNode: null,
        scriptProcessor: null,
        scriptBuffer: new Float32Array(0),
        analyserNode: null,
        gainNode: null,

        // Buffer tracking (main thread mirror of worklet buffer)
        bufferDuration: 0,       // ms of audio queued for playback
        isPlaying: false,
        startedPlayback: false,
        tabHidden: false,
        lastBufferUpdateTime: 0, // performance.now() timestamp of last buffer drain

        // Audio playback mode
        playbackMode: null,      // 'worklet' | 'scriptprocessor' | null

        // Frame sequence tracking (server prepends u32 LE seq_no to each binary frame)
        lastSeqNo: null,         // last sequence number received
        missedFrames: 0,         // count of frames skipped due to gaps

        // Stats
        framesReceived: 0,
        framesPlayed: 0,
        underruns: 0,
        overflows: 0,

        // UI
        animationFrame: null,
        lastUiUpdate: 0
    };

    // =============================================================================
    // DOM Elements
    // =============================================================================
    const els = {};

    function cacheElements() {
        els.statusIndicator = document.getElementById('statusIndicator');
        els.statusText = document.getElementById('statusText');
        els.connectBtn = document.getElementById('connectBtn');
        els.btnText = els.connectBtn.querySelector('.btn-text');
        els.btnLoader = els.connectBtn.querySelector('.btn-loader');
        els.audioIndicator = document.getElementById('audioIndicator');
        els.audioLevelBars = document.getElementById('audioLevelBars');
        els.bufferIndicator = document.getElementById('bufferIndicator');
        els.bufferFill = document.getElementById('bufferFill');
        els.bufferText = document.getElementById('bufferText');
        els.messagePanel = document.getElementById('messagePanel');
        els.messageText = document.getElementById('messageText');
        els.dismissBtn = document.getElementById('dismissBtn');
        els.wsUrl = document.getElementById('wsUrl');
        els.statsReceived = document.getElementById('statsReceived');
        els.statsPlayed = document.getElementById('statsPlayed');
        els.statsUnderruns = document.getElementById('statsUnderruns');
    }

    // =============================================================================
    // Utility Functions
    // =============================================================================
    function log(...args) {
        console.log('[SoundShared]', new Date().toISOString(), ...args);
    }

    function error(...args) {
        console.error('[SoundShared]', new Date().toISOString(), ...args);
    }

    function setStatus(status, text) {
        const indicator = els.statusIndicator;
        const textEl = els.statusText;

        indicator.classList.remove('connecting', 'connected', 'error', 'disconnected');
        textEl.classList.remove('connecting', 'connected', 'error', 'disconnected');

        if (status !== 'disconnected') {
            indicator.classList.add(status);
            textEl.classList.add(status);
        }

        textEl.textContent = text || status.charAt(0).toUpperCase() + status.slice(1);
    }

    function setConnecting(isConnecting) {
        state.isConnecting = isConnecting;
        els.connectBtn.disabled = isConnecting;
        els.btnText.hidden = isConnecting;
        els.btnLoader.hidden = !isConnecting;

        if (!isConnecting) {
            if (els.connectBtn.classList.contains('disconnected')) {
                els.connectBtn.textContent = 'Connect & Listen';
            } else {
                els.connectBtn.textContent = 'Disconnect';
            }
            els.connectBtn.appendChild(els.btnLoader);
            els.connectBtn.insertBefore(els.btnText, els.btnLoader);
        }
    }

    function setButtonDisconnected() {
        els.connectBtn.textContent = 'Connect & Listen';
        els.connectBtn.disabled = false;
        els.btnText.hidden = false;
        els.btnLoader.hidden = true;
        els.connectBtn.appendChild(els.btnLoader);
        els.connectBtn.insertBefore(els.btnText, els.btnLoader);
    }

    function setButtonConnected() {
        els.connectBtn.textContent = 'Disconnect';
        els.connectBtn.disabled = false;
        els.btnText.hidden = false;
        els.btnLoader.hidden = true;
        els.connectBtn.appendChild(els.btnLoader);
        els.connectBtn.insertBefore(els.btnText, els.btnLoader);
    }

    function showAudioIndicator(show) {
        els.audioIndicator.hidden = !show;
        state.isPlaying = show;
    }

    function updateBufferUI() {
        const ms = state.bufferDuration;
        const pct = Math.min(100, (ms / CONFIG.maxBufferMs) * 100);

        els.bufferFill.style.width = pct + '%';
        els.bufferText.textContent = ms.toFixed(0) + ' ms buffered';

        // Color coding
        els.bufferFill.classList.remove('buffer-low', 'buffer-ok', 'buffer-high');
        if (ms < CONFIG.lowBufferMs) {
            els.bufferFill.classList.add('buffer-low');
        } else if (ms > CONFIG.maxBufferMs * 0.8) {
            els.bufferFill.classList.add('buffer-high');
        } else {
            els.bufferFill.classList.add('buffer-ok');
        }
    }

    function showMessage(message, isError = false, showDismiss = true) {
        els.messageText.textContent = message;
        els.messagePanel.hidden = false;
        els.dismissBtn.hidden = !showDismiss;
        els.messagePanel.style.borderLeftColor = isError ? 'var(--accent-error)' : 'var(--accent-warning)';
    }

    function hideMessage() {
        els.messagePanel.hidden = true;
    }

    function buildWsUrl() {
        const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
        const host = location.host || 'localhost:8080';
        return `${protocol}//${host}/ws/audio`;
    }

    function sleep(ms) {
        return new Promise(resolve => setTimeout(resolve, ms));
    }

    // =============================================================================
    // Audio Playback - AudioWorkletNode (runs on audio thread)
    // =============================================================================
    async function initAudio() {
        if (state.audioContext) {
            if (state.audioContext.state === 'suspended') {
                await state.audioContext.resume();
            }
            return;
        }

        try {
            state.audioContext = new (window.AudioContext || window.webkitAudioContext)({
                sampleRate: CONFIG.sampleRate
            });
            log('AudioContext created, sampleRate:', state.audioContext.sampleRate);

            // Create AnalyserNode for real-time visualization
            state.analyserNode = state.audioContext.createAnalyser();
            state.analyserNode.fftSize = 256;
            state.analyserNode.smoothingTimeConstant = 0.6;

            // Create GainNode for volume control
            state.gainNode = state.audioContext.createGain();
            state.gainNode.gain.value = 1.0;

            // Try AudioWorklet first (runs on audio thread, no main-thread blocking)
            try {
                const blob = new Blob([WORKLET_CODE], { type: 'application/javascript' });
                const blobUrl = URL.createObjectURL(blob);
                await state.audioContext.audioWorklet.addModule(blobUrl);
                URL.revokeObjectURL(blobUrl);

                state.audioWorkletNode = new AudioWorkletNode(
                    state.audioContext,
                    'audio-stream-processor'
                );

                // Connect: worklet -> analyser -> gain -> destination
                state.audioWorkletNode.connect(state.analyserNode);
                state.analyserNode.connect(state.gainNode);
                state.gainNode.connect(state.audioContext.destination);

                state.playbackMode = 'worklet';
                log('AudioWorkletNode + AnalyserNode + GainNode connected');
            } catch (workletErr) {
                // AudioWorklet not available — fall back to ScriptProcessorNode
                error('AudioWorklet unavailable, falling back to ScriptProcessorNode:', workletErr);
                state.audioWorkletNode = null;

                const bufferSize = 4096;
                state.scriptProcessor = state.audioContext.createScriptProcessor(bufferSize, 2, 2);

                state.scriptProcessor.onaudioprocess = (event) => {
                    const outputL = event.outputBuffer.getChannelData(0);
                    const outputR = event.outputBuffer.getChannelData(1);
                    const framesToFill = outputL.length;
                    const samplesToFill = framesToFill * 2;

                    let samplesFilled = 0;

                    while (samplesFilled < samplesToFill && state.scriptBuffer.length >= 2) {
                        const samplesToCopy = Math.min(samplesToFill - samplesFilled, state.scriptBuffer.length);
                        const framesToCopy = Math.floor(samplesToCopy / 2);
                        const samplesToCopyActual = framesToCopy * 2;

                        for (let i = 0; i < framesToCopy; i++) {
                            const outIdx = samplesFilled / 2 + i;
                            outputL[outIdx] = state.scriptBuffer[samplesFilled + i * 2];
                            outputR[outIdx] = state.scriptBuffer[samplesFilled + i * 2 + 1];
                        }

                        samplesFilled += samplesToCopyActual;

                        if (samplesToCopyActual < state.scriptBuffer.length) {
                            state.scriptBuffer = state.scriptBuffer.slice(samplesToCopyActual);
                        } else {
                            state.scriptBuffer = new Float32Array(0);
                        }
                    }

                    // Fill remaining with silence
                    for (let i = samplesFilled / 2; i < framesToFill; i++) {
                        outputL[i] = 0;
                        outputR[i] = 0;
                    }
                };

                state.scriptProcessor.connect(state.analyserNode);
                state.analyserNode.connect(state.gainNode);
                state.gainNode.connect(state.audioContext.destination);

                state.playbackMode = 'scriptprocessor';
                log('ScriptProcessorNode + AnalyserNode + GainNode connected (fallback)');
            }

            log('Audio initialized, mode:', state.playbackMode);
        } catch (e) {
            error('Failed to initialize audio:', e);
            throw e;
        }
    }

    function queueAudioData(float32Array) {
        // float32Array is interleaved stereo: [L, R, L, R, ...]
        // Drop audio while tab is hidden — the audio thread isn't consuming,
        // and updateUI isn't draining, so data would just pile up and overflow.
        if (state.tabHidden) {
            return;
        }

        const frameCount = float32Array.length / 2;
        const durationMs = frameCount / CONFIG.sampleRate * 1000;

        state.bufferDuration += durationMs;
        state.framesReceived += frameCount;
        updateStatsUI();

        // Send to audio processor (worklet or script processor)
        if (state.playbackMode === 'worklet' && state.audioWorkletNode) {
            state.audioWorkletNode.port.postMessage({
                channelData: float32Array
            });
        } else if (state.playbackMode === 'scriptprocessor' && state.scriptProcessor) {
            // Append to script processor buffer
            const combined = new Float32Array(state.scriptBuffer.length + float32Array.length);
            combined.set(state.scriptBuffer);
            combined.set(float32Array, state.scriptBuffer.length);
            state.scriptBuffer = combined;
        }

        // Prevent unbounded buffer growth
        if (state.bufferDuration > CONFIG.maxBufferMs) {
            state.overflows++;
            log('Buffer overflow, queueDuration:', state.bufferDuration.toFixed(1), 'ms');
            updateStatsUI();
        }

        // Auto-start playback when we have enough buffered
        if (!state.startedPlayback && state.bufferDuration >= CONFIG.targetBufferMs) {
            state.startedPlayback = true;
            state.lastBufferUpdateTime = performance.now();
            showAudioIndicator(true);
            log('Playback started, buffered:', state.bufferDuration.toFixed(1), 'ms');
        }

        updateBufferUI();
    }

    /// Insert silence for missed frames to maintain smooth playback
    function insertSilenceForMissedFrames(missedCount) {
        if (missedCount <= 0) return;
        const samplesPerFrame = CONFIG.sampleRate * CONFIG.channels * 0.01; // 10 ms per frame
        const silenceSamples = Math.floor(samplesPerFrame * missedCount);
        const silence = new Float32Array(silenceSamples);

        state.bufferDuration += silenceSamples / CONFIG.sampleRate * 1000;
        state.missedFrames += missedCount;
        state.framesReceived += silenceSamples / 2;
        updateStatsUI();

        if (state.playbackMode === 'worklet' && state.audioWorkletNode) {
            state.audioWorkletNode.port.postMessage({
                channelData: silence
            });
        } else if (state.playbackMode === 'scriptprocessor' && state.scriptProcessor) {
            const combined = new Float32Array(state.scriptBuffer.length + silence.length);
            combined.set(state.scriptBuffer);
            combined.set(silence, state.scriptBuffer.length);
            state.scriptBuffer = combined;
        }

        updateBufferUI();
    }

    function clearAudioQueue() {
        state.bufferDuration = 0;
        state.startedPlayback = false;
        state.lastBufferUpdateTime = 0;
        state.lastSeqNo = null;
        state.missedFrames = 0;
        state.underruns = 0;
        state.overflows = 0;
        state.framesReceived = 0;
        state.framesPlayed = 0;
        state.scriptBuffer = new Float32Array(0);
        showAudioIndicator(false);
        updateBufferUI();
        updateStatsUI();
    }

    // =============================================================================
    // UI Update Loop (runs on main thread, reads buffer state)
    // =============================================================================
    const analyserData = new Uint8Array(128);

    function updateUI() {
        const now = performance.now();

        if (!state.audioContext || state.audioContext.state !== 'running') {
            // Still schedule next frame if we might start soon
            state.animationFrame = requestAnimationFrame(updateUI);
            return;
        }

        // Drain bufferDuration based on elapsed time (audio is being played by the context)
        if (state.startedPlayback && state.lastBufferUpdateTime > 0) {
            const elapsedMs = now - state.lastBufferUpdateTime;
            if (elapsedMs > 0) {
                state.bufferDuration = Math.max(0, state.bufferDuration - elapsedMs);
                state.lastBufferUpdateTime = now;

                // Estimate frames played based on elapsed time
                const framesPlayed = Math.floor(elapsedMs / 1000 * CONFIG.sampleRate);
                state.framesPlayed += framesPlayed;
                updateStatsUI();
            }
        }

        // Get real-time audio levels from AnalyserNode
        if (state.analyserNode) {
            state.analyserNode.getByteFrequencyData(analyserData);

            let sum = 0;
            for (let i = 0; i < analyserData.length; i++) {
                sum += analyserData[i];
            }
            const avg = sum / analyserData.length;
            const level = avg / 255; // 0 to 1

            // Update audio level bars
            const bars = els.audioLevelBars.children;
            const barCount = bars.length;
            for (let i = 0; i < barCount; i++) {
                const bar = bars[i];
                const bandIdx = Math.floor(i * analyserData.length / barCount);
                const barLevel = analyserData[bandIdx] / 255;
                const height = Math.max(10, barLevel * 100);
                bar.style.height = height + '%';
                bar.style.opacity = Math.min(1, barLevel + 0.3);
            }

            // Show/hide audio indicator based on activity
            if (state.isPlaying && level > 0.01) {
                els.audioIndicator.hidden = false;
            }
        }

        // Update buffer UI
        updateBufferUI();

        // Check for underrun (buffer empty while we should be playing)
        if (state.startedPlayback && state.bufferDuration < CONFIG.lowBufferMs) {
            if (state.bufferDuration <= 0) {
                state.underruns++;
                updateStatsUI();
            }
        }

        // Schedule next update
        state.animationFrame = requestAnimationFrame(updateUI);
    }

    function updateStatsUI() {
        if (els.statsReceived) els.statsReceived.textContent = state.framesReceived.toLocaleString();
        if (els.statsPlayed) els.statsPlayed.textContent = state.framesPlayed.toLocaleString();
        if (els.statsUnderruns) els.statsUnderruns.textContent = state.underruns.toLocaleString();
        const elsMissed = document.getElementById('statsMissed');
        if (elsMissed) elsMissed.textContent = state.missedFrames.toLocaleString();
    }

    // =============================================================================
    // WebSocket Connection
    // =============================================================================
    function connect() {
        if (state.ws && (state.ws.readyState === WebSocket.OPEN || state.ws.readyState === WebSocket.CONNECTING)) {
            log('Already connected or connecting');
            return;
        }

        state.wsUrl = buildWsUrl();
        els.wsUrl.textContent = state.wsUrl;
        state.shouldReconnect = true;
        state.reconnectAttempt = 0;

        doConnect();
    }

    function doConnect() {
        if (!state.shouldReconnect) return;

        state.isConnecting = true;
        setStatus('connecting', 'Connecting...');
        setConnecting(true);
        hideMessage();

        log('Connecting to:', state.wsUrl);

        try {
            state.ws = new WebSocket(state.wsUrl);
            state.ws.binaryType = 'arraybuffer';

            state.ws.onopen = onWsOpen;
            state.ws.onmessage = onWsMessage;
            state.ws.onclose = onWsClose;
            state.ws.onerror = onWsError;
        } catch (e) {
            error('WebSocket creation failed:', e);
            handleConnectionError(e);
        }
    }

    function onWsOpen() {
        log('WebSocket connected');
        state.isConnecting = false;
        state.reconnectAttempt = 0;
        setStatus('connected', 'Connected');
        setButtonConnected();
        clearAudioQueue();
    }

    function onWsMessage(event) {
        if (event.data instanceof ArrayBuffer) {
            // Binary audio data format: [seq_no: u32 LE][pcm_samples: f32 LE ...]
            const data = new DataView(event.data);
            const seqNo = data.getUint32(0, true); // little-endian

            // Detect gaps in sequence numbers
            if (state.lastSeqNo !== null) {
                const gap = seqNo - state.lastSeqNo - 1;
                if (gap > 0) {
                    log('Frame gap detected, missed', gap, 'frame(s) after seq', state.lastSeqNo);
                    insertSilenceForMissedFrames(gap);
                }
            }
            state.lastSeqNo = seqNo;

            // Skip the 4-byte sequence header, remaining bytes are PCM
            const pcmBytes = event.data.byteLength - 4;
            const float32Array = new Float32Array(event.data, 4, pcmBytes / 4);
            queueAudioData(float32Array);
        } else if (typeof event.data === 'string') {
            try {
                const msg = JSON.parse(event.data);
                handleControlMessage(msg);
            } catch (e) {
                log('Received text message:', event.data);
            }
        }
    }

    function handleControlMessage(msg) {
        log('Control message:', msg);

        switch (msg.type) {
            case 'format':
                log('Format confirmed:', msg);
                break;
            case 'error':
                showMessage(msg.message || 'Server error', true);
                break;
            case 'info':
                showMessage(msg.message || 'Info from server', false);
                break;
            case 'welcome':
                log('Welcome from server');
                break;
            default:
                log('Unknown message type:', msg.type);
        }
    }

    function onWsClose(event) {
        log('WebSocket closed:', event.code, event.reason);
        state.isConnecting = false;

        state.audioWorkletNode = null;
        state.analyserNode = null;
        state.gainNode = null;

        if (state.shouldReconnect) {
            if (event.code === 1000) {
                setStatus('disconnected', 'Disconnected');
                setButtonDisconnected();
                showMessage('Disconnected from server', false, true);
            } else {
                setStatus('disconnected', 'Disconnected - Reconnecting...');
                setButtonDisconnected();
                scheduleReconnect(event.reason || `Code ${event.code}`);
            }
        } else {
            setStatus('disconnected', 'Disconnected');
            setButtonDisconnected();
        }

        state.ws = null;
        showAudioIndicator(false);
        clearAudioQueue();
    }

    function onWsError(event) {
        error('WebSocket error:', event);
        // onclose will be called after this
    }

    function handleConnectionError(err) {
        state.isConnecting = false;
        setButtonDisconnected();
        setStatus('error', 'Connection Error');
        showMessage(`Failed to connect: ${err.message}`, true, true);
    }

    function scheduleReconnect(reason) {
        if (!state.shouldReconnect) return;

        state.reconnectAttempt++;
        const { initialDelay, maxDelay, factor, jitter, maxAttempts } = CONFIG.reconnect;

        if (state.reconnectAttempt > maxAttempts) {
            log('Max reconnect attempts reached');
            setStatus('error', 'Connection Failed');
            showMessage('Max reconnection attempts reached. Please refresh the page.', true, true);
            return;
        }

        const baseDelay = Math.min(initialDelay * Math.pow(factor, state.reconnectAttempt - 1), maxDelay);
        const jitterAmount = baseDelay * jitter * Math.random();
        const delay = Math.floor(baseDelay + jitterAmount);

        log(`Reconnect attempt ${state.reconnectAttempt} in ${delay}ms (reason: ${reason})`);
        showMessage(`Reconnecting in ${(delay / 1000).toFixed(1)}s... (attempt ${state.reconnectAttempt})`, false, false);

        if (state.reconnectTimer) clearTimeout(state.reconnectTimer);
        state.reconnectTimer = setTimeout(() => {
            if (state.shouldReconnect) {
                doConnect();
            }
        }, delay);
    }

    function disconnect() {
        log('Manual disconnect');
        state.shouldReconnect = false;

        if (state.reconnectTimer) {
            clearTimeout(state.reconnectTimer);
            state.reconnectTimer = null;
        }

        if (state.animationFrame) {
            cancelAnimationFrame(state.animationFrame);
            state.animationFrame = null;
        }

        if (state.ws) {
            state.ws.close(1000, 'User disconnected');
            state.ws = null;
        }

        // Suspend audio context
        if (state.audioContext && state.audioContext.state === 'running') {
            state.audioContext.suspend().catch(() => {});
        }

        setStatus('disconnected', 'Disconnected');
        setButtonDisconnected();
        showAudioIndicator(false);
        clearAudioQueue();
        hideMessage();
    }

    // =============================================================================
    // Event Handlers
    // =============================================================================
    async function onConnectClick() {
        if (state.isConnecting) return;

        // Toggle: if connected, disconnect; otherwise connect
        if (state.ws && state.ws.readyState === WebSocket.OPEN) {
            disconnect();
            return;
        }

        try {
            // Initialize audio context on user interaction (autoplay policy)
            await initAudio();

            if (state.audioContext.state === 'running') {
                connect();
            } else {
                await state.audioContext.resume();
                connect();
            }
        } catch (e) {
            error('Failed to initialize audio:', e);
            setStatus('error', 'Audio Error');
            showMessage(`Failed to initialize audio: ${e.message}. Please check browser permissions.`, true, true);
        }
    }

    function onDismissClick() {
        hideMessage();
    }

    function onVisibilityChange() {
        state.tabHidden = document.hidden;
        if (document.hidden && state.ws && state.ws.readyState === WebSocket.OPEN) {
            log('Tab hidden, keeping connection alive');
        } else if (!document.hidden) {
            // Tab became visible: resume audio context if needed and clear stale buffer
            if (state.audioContext && state.audioContext.state === 'suspended') {
                state.audioContext.resume().catch(() => {});
            }
            // Drop any data that accumulated while hidden to avoid a burst of old audio
            clearAudioQueue();
            log('Tab visible, buffer cleared');
        }
    }

    function onBeforeUnload() {
        disconnect();
    }

    // =============================================================================
    // Initialization
    // =============================================================================
    function init() {
        cacheElements();

        els.wsUrl.textContent = buildWsUrl();

        // Event listeners
        els.connectBtn.addEventListener('click', onConnectClick);
        els.dismissBtn.addEventListener('click', onDismissClick);
        document.addEventListener('visibilitychange', onVisibilityChange);
        window.addEventListener('beforeunload', onBeforeUnload);

        // Check autoplay policy (non-fatal hint)
        try {
            const testCtx = new (window.AudioContext || window.webkitAudioContext)();
            if (testCtx.state === 'suspended') {
                testCtx.close().catch(() => {});
                log('AudioContext starts suspended - user interaction required');
            }
        } catch (e) {
            log('AudioContext check failed (non-fatal):', e);
        }

        // Start UI update loop
        state.animationFrame = requestAnimationFrame(updateUI);

        // Enable Connect button initially
        setButtonDisconnected();

        log('SoundShared Listener initialized');
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }

    // Expose for debugging
    window.SoundShared = {
        getState: () => ({ ...state }),
        connect,
        disconnect
    };
})();
