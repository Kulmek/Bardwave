# Shared UI Primitives (SoundShared Listener)

Source: `src/network/static/index.html`, `src/network/static/style.css`, `src/network/static/app.js`

## 1. Button (`.btn`, `.btn-primary`, `.btn-secondary`)
- Base: inline-flex, centered text, gap 8px, padding 14px 28px, font-weight 600, border-radius 10px, min-width 220px
- Primary: linear-gradient 135deg from accent-primary to accent-secondary, shadow, hover lift + shadow
- Secondary: bg-tertiary background, 1px border, hover bg-border
- Loader: spinning 20px border inside button, toggled by JS on connect/disconnect states
- States: disabled (opacity 0.5, not-allowed), focus-visible outline 2px accent-primary

## 2. Status Indicator (`.status-indicator`, `.status-text`)
- 12px circle, 50% border-radius, box-shadow 0 0 0 2px bg-secondary
- States: connecting (warning color, pulse-ring animation), connected (success color + glow), error (error color + pulse-error), disconnected (muted color)
- Text: font-weight 600, color tracks state color

## 3. Audio Level Meter (`.audio-panel`, `.audio-level-bars`, `.level-bar`)
- Hidden by default, shown when playback starts
- 8 flex bars, height animated via JS from AnalyserNode frequency data
- Gradient fill accent-primary → accent-secondary, origin bottom

## 4. Buffer Health Bar (`.buffer-panel`, `.buffer-bar`, `.buffer-fill`)
- Horizontal progress bar, height 6px, fill color-coded: low=error, ok=primary, high=warning
- Text shows ms buffered in monospace

## 5. Message Panel (`.message-panel`, `.message-content`, `.message-icon`, `#dismissBtn`)
- Hidden by default, slide-down animation
- Left border color: error (accent-error) or warning (accent-warning)
- SVG icon (circle + line), dismiss button aligned right

## 6. Info Grid (`.info-panel`, `.info-grid`, `.info-item`, `.info-label`, `.info-value`)
- `<details>/<summary>` collapsible section
- 2-column grid (1-col on mobile), label uppercase 0.7rem muted, value monospace 0.875rem

## 7. Stats Panel (`.stats-panel`, `.stat-item`, `.stat-label`, `.stat-value`)
- 3-column grid (1-col on mobile): Received / Played / Underruns
- Label uppercase muted, value monospace bold, min-width 0 + overflow hidden + ellipsis + nowrap

## 8. Container / Header / Footer (`.container`, `header`, `footer`)
- Max-width 560px centered, padding 24px 16px
- Header: centered h1 with accent span, subtitle muted
- Footer: centered muted text 0.8rem