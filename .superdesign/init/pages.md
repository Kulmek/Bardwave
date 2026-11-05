# Page Dependency Trees (SoundShared Listener)

Source: `src/network/static/index.html`, `src/network/static/style.css`, `src/network/static/app.js`

## Page: Listener (`/`)
- HTML: `src/network/static/index.html` — single page, all sections inline
- CSS: `src/network/static/style.css` — Red Noir tokens, 12-column layout, responsive + reduced-motion queries
- JS: `src/network/static/app.js` — WebSocket connection, AudioWorklet jitter-buffered playback, ScriptProcessor fallback, buffer/underrun stats, analyser meter, UI state machine
- External assets:
  - Iconify web component (`https://code.iconify.design/iconify-icon/1.0.7/iconify-icon.min.js`) for Lucide/MDI icons
  - Google Fonts import for Archivo + JetBrains Mono (system fallbacks keep the page usable offline)
- No frameworks, no Tailwind build step, no React/Vue
- AudioWorklet: inline Blob code injected at runtime via `WORKLET_CODE`
- Server embedding: `src/network/server.rs` uses `include_str!` for index.html, style.css, app.js
