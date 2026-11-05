# Shared UI Primitives (SoundShared Listener — Red Noir)

Source: `src/network/static/index.html`, `src/network/static/style.css`, `src/network/static/app.js`

## 1. Button (`.btn`, `.btn-primary`)
- Inline-flex, centered, gap 0.75rem, padding 1rem 2.5rem, square corners
- Rose-600 background, hover rose-700, white uppercase 900 text with 0.1em tracking
- `.btn-sheen`: white/10 overlay that slides up on hover
- `.btn-loader`: 18px spinning ring, toggled by JS while connecting
- States: disabled (opacity .7, wait cursor), focus-visible white outline

## 2. Status Chip (`.status-chip`, `.status-copy`, `.status-dot`, `.status-text`)
- Header chip: translucent zinc-900/50, 1px white/5 border, 2px radius
- 12px dot + stacked label/value aligned right
- States: disconnected (rose-600 pulse), connecting (amber-400 pulse), connected (rose-500 glow), error (rose-400 fast pulse)
- `aria-live="polite"` on the chip

## 3. Audio Level Meter (`.meter`, `.level-track`, `.level-fill`, `.level-peak`)
- Label + dB value header, 16px track on zinc-900
- Fill: gradient rose-950 → rose-600, width driven by RMS dB from AnalyserNode
- Peak marker: 2px white/20 line with peak hold + decay
- `.is-clipping` swaps fill to rose-600 → rose-400

## 4. Buffer Health Grid (`.buffer-grid`, `.buffer-cell`)
- 20 equal columns, 4px gap, 8px tall
- Cells fill left-to-right from `availableFrames / targetBufferMs`
- Active = rose-600; warn (above 75% of max) = amber-400
- `.is-low` makes active cells rose-400 and pulses the whole grid

## 5. Toast Panel (`.toast-panel`, `.toast`, `.toast--error/info/success/idle`)
- Stacked cards on zinc-900 with a 4px left border
- Severity border: rose-600 (error/info), rose-500 (success), zinc-700 (idle)
- Iconify icon + text + optional dismiss button
- `.toast--idle.is-active` promotes the idle system toast to full opacity

## 6. Info Grid (`.info-panel`, `.info-summary`, `.info-grid`, `.info-row`)
- `<details open>` collapsible panel, zinc-900/20 background
- Summary row with chevron rotating 180° when open
- Rows: label left (zinc-500), monospace value right, hairline separators

## 7. Stats Panel (`.stats`, `.stat-card`, `.stat-label`, `.stat-value`)
- 3-column grid at ≥768px, single column below
- zinc-900/40 cards, 1px white/5 border
- `.stat-card--accent`: 2px rose-600/50 left border for Frames Played
- Monospace values, nowrap + ellipsis overflow protection

## 8. Page Shell (`.page`, `.site-header`, `.site-footer`)
- Max-width 72rem, centered, 3rem 1.5rem padding
- Header: brand block + status chip, bottom hairline
- Footer: docs/GitHub links, copyright, social icons; wraps on mobile
- `.noise-overlay`: fixed inline-SVG grain at 5% opacity, pointer-events none
