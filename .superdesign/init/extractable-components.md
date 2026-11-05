# Extractable Components (SoundShared Listener)

Source: `src/network/static/index.html`, `src/network/static/style.css`

Components suitable for extraction as reusable DraftComponents in Superdesign:

1. **StatusChip** — `.status-indicator` + `.status-text`, 4 states (connecting/connected/error/disconnected) with color + animation
2. **PrimaryButton** — `.btn-primary`, gradient bg, hover lift, loader slot, disabled state
3. **SecondaryButton** — `.btn-secondary`, outline style, hover fill
4. **AudioMeter** — `.audio-level-bars` + 8 `.level-bar` bars, JS-driven height/opacity
5. **BufferBar** — `.buffer-panel` with label, progress fill, color-coded (low/ok/high), ms text
6. **MessageToast** — `.message-panel`, slide-down, left border color by severity, dismiss button
7. **StatTile** — `.stat-item` with label + value, monospace value, overflow protection
8. **InfoCard** — `.info-panel` details/summary with 2-col info grid
9. **ConnectionPanel** — composite: StatusChip + PrimaryButton + AudioMeter + BufferBar
10. **StatsPanel** — 3× StatTile grid (Received/Played/Underruns)