# Shared Layout Components (SoundShared Listener — Red Noir)

Source: `src/network/static/index.html`, `src/network/static/style.css`

## Layout: Centered 12-column console
- `.page`: flex column, centered, min-height 100vh, padding 3rem 1.5rem
- `.site-header`: full width, max 72rem, brand + status chip, bottom hairline, margin-bottom 4rem
- `.layout`: max 72rem; 1 column below 1024px, 12 columns at ≥1024px
- `.layout-main`: spans 8 columns on desktop (connection panel + stats)
- `.layout-aside`: spans 4 columns on desktop (info panel + toasts)
- `.site-footer`: max 72rem, margin-top auto, top hairline, 3-part row on desktop

## Layout sections (top to bottom)
1. `<header>` — kicker, `Listener.v2` title, subtitle, status chip
2. `.connection-panel` — panel head + connect button, level meter, buffer grid, red scanline overlay
3. `.stats` — three stat cards (Received / Played / Underruns)
4. `.info-panel` — collapsible connection details
5. `.toast-panel` — error/info toast + idle system toast
6. `<footer>` — links, copyright, social icons

## Responsive breakpoints
- ≥768px: header row, panel head row, stats 3 columns, footer row
- ≥1024px: 12-column layout with 8/4 main/aside split
- ≤480px: page padding 2rem 1rem, panel padding 1.5rem, full-width button, stacked header/status chip, tighter buffer gap, smaller stat values
- `prefers-reduced-motion`: animations and transitions reduced to 0.01ms
