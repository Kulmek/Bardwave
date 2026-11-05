# Routes / Page Mapping (SoundShared Listener)

Source: `src/network/static/index.html`, `src/network/server.rs`, `src/main.rs`

This is a **single-page application** (static listener page). There is no client-side routing.

## Client page
| Route | File | Description |
|-------|------|-------------|
| `/` | `src/network/static/index.html` | SoundShared Listener — Red Noir connection console, level meter, buffer health, stats, connection details, toasts |

## Static assets (embedded via `include_str!`)
| Route | File |
|-------|------|
| `/index.html` | `src/network/static/index.html` |
| `/style.css` | `src/network/static/style.css` |
| `/app.js` | `src/network/static/app.js` |

## HTTP / WebSocket endpoints
| Route | Purpose |
|-------|---------|
| `/api/status` | JSON status: running state, listener count, audio format |
| `/ws/audio` | WebSocket audio stream: `[seq_no: u32 LE][f32 PCM LE interleaved stereo 48 kHz]` |

No nested client routes, no lazy-loaded pages, no server-side rendered routes beyond the listener page.
