# Design Tokens (SoundShared Listener — Red Noir)

Source: `src/network/static/style.css` `:root` variables

## Color palette
| Token | Value | Role |
|-------|-------|------|
| --noir-black | #0A0A0A | Page background |
| --noir-panel | #111111 | Primary panel background |
| --noir-zinc-950 | #09090B | Deepest surface |
| --noir-zinc-900 | #18181B | Elevated surface / toast background |
| --noir-zinc-800 | #27272A | Borders / inactive toast edge |
| --noir-zinc-700 | #3F3F46 | Strong border |
| --noir-zinc-600 | #52525B | Footer copy |
| --noir-zinc-500 | #71717A | Muted / label text |
| --noir-zinc-400 | #A1A1AA | Secondary text |
| --noir-zinc-300 | #D4D4D8 | Toast body text |
| --rose-400 | #FB7185 | Error / clipping accent |
| --rose-500 | #F43F5E | Connected accent |
| --rose-600 | #E11D48 | Primary accent (brand red) |
| --rose-700 | #BE123C | Primary button hover |
| --rose-950 | #4C0519 | Meter gradient start / buffer cell base |
| --amber-400 | #FBBF24 | Connecting / buffer warning |
| --white | #FFFFFF | Primary text |
| --white-5 / -10 / -20 | rgba(255,255,255,.05/.10/.20) | Hairlines, borders, peak marker |

## Typography
- Sans: 'Archivo' (400/500/700/900) with system fallbacks
- Mono: 'JetBrains Mono' (400/700) for stats, URLs, meters
- Brand title: clamp(2.75rem, 8vw, 4.5rem), weight 900, uppercase, tight tracking
- Panel title: 1.5rem, weight 900, uppercase
- Labels: 10–11px, uppercase, wide tracking
- Body/subtitle: 0.875–1rem, zinc-500

## Spacing / Radius
- Page max width: 72rem (1152px, Tailwind max-w-6xl)
- Panel padding: 2rem (1.5rem below 480px)
- Layout gap: 2rem
- Radius: 2px (`--radius-sm`); buttons and buffer cells are square

## Transitions / Animation
- --transition-fast: 150ms ease
- --transition-normal: 250ms ease
- --transition-slow: 300ms ease
- Keyframes: pulse-dot, scan (red scanline), spin (loader), buffer-warning, toast-in
- `prefers-reduced-motion` disables animations/transitions
