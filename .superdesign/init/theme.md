# Design Tokens (SoundShared Listener)

Source: `src/network/static/style.css` `:root` variables

## Color palette (current)
| Token | Value | Role |
|-------|-------|------|
| --bg-primary | #0f172a | Page background |
| --bg-secondary | #1e293b | Panel backgrounds |
| --bg-tertiary | #334155 | Elevated surfaces, buffer bar bg |
| --fg-primary | #f8fafc | Primary text |
| --fg-secondary | #94a3b8 | Secondary text |
| --fg-muted | #64748b | Muted / label text |
| --accent-primary | #22d3ee | Primary accent (cyan) |
| --accent-secondary | #818cf8 | Secondary accent (indigo) |
| --accent-warning | #fbbf24 | Warning / connecting |
| --accent-error | #f87171 | Error / low buffer |
| --accent-success | #34d399 | Success / connected |
| --border-color | #334155 | Borders |
| --shadow-color | rgba(0,0,0,0.4) | Drop shadows |

## Typography
- System font stack: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif
- Monospace: 'SF Mono', Monaco, 'Cascadia Code', monospace (stats, buffer text)
- Sizes: 0.7rem labels, 0.75rem small, 0.875rem subtitle/small value, 0.9rem secondary btn, 1rem primary text, 1.125rem stat value, 1.75rem h1

## Spacing / Radius
- --radius-sm: 6px, --radius-md: 10px, --radius-lg: 16px
- Panel padding: 28px 24px (connection), 20px 24px (message), 16px 20px (info), 20px 16px (stats)

## Transitions / Animation
- --transition-fast: 150ms ease, --transition-normal: 250ms ease
- Keyframes: pulse-ring, pulse-error, spin, fade-in, slide-down