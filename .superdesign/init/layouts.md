# Shared Layout Components (SoundShared Listener)

Source: `src/network/static/index.html`, `src/network/static/style.css`

## Layout: Single-column centered container
- `.container`: max-width 560px, margin 0 auto, padding 24px 16px, flex column, min-height 100vh via body
- Sections stack vertically with `gap: 16px` inside `main`

## Layout sections (top to bottom)
1. `<header>` — h1 title + .accent span, .subtitle
2. `.connection-panel` — status indicator + text, connect/disconnect button, audio level meter, buffer bar
3. `.message-panel` — hidden by default, slide-down
4. `.info-panel` — collapsible details/summary with connection info grid
5. `.stats-panel` — 3 stat items grid
6. `<footer>` — centered attribution

## Responsive breakpoints
- max-width 480px: container padding 16px 12px, h1 1.5rem, btn-primary full width, info-grid 1-col, stats-panel 1-col gap 8px padding 16px 12px, audio bars height 32px