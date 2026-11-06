# Opus wire migration (F32 PCM → Opus)

SoundShared streams the 48 kHz stereo f32 mix to browser listeners over WebSocket. Raw f32 PCM at 48 kHz stereo is 3 Mbit/s — it floods weak Wi-Fi, bursts the client jitter buffer (overruns), and burns CPU on every listener. The wire now carries 10 ms Opus packets (~100–300 bytes at 64 kbit/s stereo VBR) instead.

## What changed

### `src/network/opus_wire.rs` (new)
- `WireEncoder` wraps `opus::Encoder` (48 kHz, stereo, `Application::Audio`, 64 kbit/s VBR) plus an interleaved f32 remainder buffer.
- `push(samples, out)` accepts *any* block size the local audio device delivers (441, 512, 1024, 47 …) and emits only whole 480-frame (10 ms) Opus packets.
- Fast path for already-aligned 480-frame blocks — zero copies in the headless mixer's steady state.
- `drop_partial_frame()` discards a half-assembled frame after a stream gap (>120 ms silence), so pre-pause audio is never glued onto the next burst — this is what kills the click on track start/stop.
- Unit tests: fixed-frame alignment, partial-frame carry across pushes, packet-size ceiling, round-trip encode→decode RMS, `drop_partial_frame` reset.

### `src/network/mod.rs`
- Registered `pub mod opus_wire;`.

### `src/network/broadcast.rs`
- `AudioBroadcaster` now holds `wire: Mutex<WireEncoder>` (opus::Encoder is `!Sync`; the lock is uncontended in single-task use).
- Every mixed block from the engine goes through `wire.push()`; completed Opus packets are queued and sent one per binary WebSocket frame: `[seq_no: u32 LE][server_ms: u32 LE][opus packet]`.
- `STREAM_RESTART_GAP` (120 ms): after a long silence the partial frame is dropped before encoding resumes.
- Format announcement unchanged in shape (`format: "opus"`, `frame_size: 480`).

### `src/network/static/app.js` (client)
- `decodeOpusPayload()` now reads `result.channelData` (the per-channel Float32Arrays that `opus-decoder.min.js` actually returns) and interleaves them into one Float32Array. Previously it read `result.samples`, which is `undefined` — decoded Opus was silence on the client.
- Wire-format comment updated: the payload is one Opus packet, not raw f32 PCM.

## What Opus does and does not fix

| Symptom | Opus effect |
|---------|-------------|
| Wire bandwidth 3 Mbit/s → ~64 kbit/s | ✅ Fixed |
| Client jitter-buffer overruns from bursty Wi-Fi | ✅ Much smaller payloads |
| Underruns rising when stopping/starting a track in GUI | ⚠️ Not a codec issue — the mixer stops publishing while stopped, the client ring drains to empty, and restart refills the 200 ms target buffer. Cure: fade-out on stop, fade-in on start, and/or lower `targetBufferMs`. |
| Click/pop at every track start | ✅ `drop_partial_frame()` prevents gluing pre-pause fragments into new packets |

## Build requirements

`rusty-opus` 1.0 — a complete pure-Rust implementation of the Opus codec (RFC 6716 / RFC 8251). No C code, no FFI, no build-time toolchain, no CMake. Builds with `cargo` alone on any host where the Rust toolchain works.

Enable with `cargo build --features opus-codec`.

## Pending (not part of this change)

- Client: no hard silence inserted for sequence gaps, fade on stall, surface lost frames in UI (jitter-buffer polish).
- Node end-to-end verification: decode real wire packets with `opus-decoder.min.js`, measure SNR/clicks.
- README: document CMake requirement + Opus wire format.
