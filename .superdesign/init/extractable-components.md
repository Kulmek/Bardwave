# Extractable Components (SoundShared Listener — Red Noir)

Source: `src/network/static/index.html`, `src/network/static/style.css`

Components suitable for extraction as reusable DraftComponents in Superdesign:

1. **StatusChip** — `.status-chip` + `.status-dot` + `.status-text`, four states (connecting/connected/error/disconnected)
2. **PrimaryButton** — `.btn-primary` with sheen, Iconify slot, and loader slot
3. **AudioMeter** — `.meter` + `.level-track` + `.level-fill` + `.level-peak`, RMS dB fill with peak hold
4. **BufferGrid** — `.buffer-grid` + 20 `.buffer-cell`, target-relative fill with low/warn states
5. **Toast** — `.toast` with severity variants (error/info/success/idle), icon, dismiss button
6. **StatTile** — `.stat-card` with label + monospace value, optional rose accent border
7. **InfoCard** — `.info-panel` details/summary with label/value rows
8. **ConnectionPanel** — composite: panel head + PrimaryButton + AudioMeter + BufferGrid + scanline
9. **StatsPanel** — 3× StatTile grid (Received / Played / Underruns)
10. **PageShell** — header brand block + StatusChip + footer links/social row
