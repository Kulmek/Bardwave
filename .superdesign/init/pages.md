# Page Dependency Trees (SoundShared Listener)

Source: `src/network/static/index.html`, `src/network/static/style.css`, `src/network/static/app.js`

## Page: Listener (`/`)
- HTML: `src/network/static/index.html` — single page, all sections inline
- CSS: `src/network/static/style.css` — all styles, responsive + reduced-motion queries
- JS: `src/network/static/app.js` — WebSocket connection, AudioWorkletNode playback, buffer tracking, UI update loop, stats
- Dependencies: none external (no frameworks, no CDN, no Tailwind, no React/Vue)
- AudioWorklet: inline Blob code injected at runtime via `WORKLET_CODE`