# SoundShared — Desktop GUI Design System

## Product context

**Product:** SoundShared is a local audio streaming server. One machine loads and mixes sound files; listeners open a browser and hear the live mix over WebSocket.

**Target surface:** the native desktop control app (egui), a single window at 1100×750 (minimum 900×600). It is the operator's control room: load sounds, play/stop them, set per-sound and master volume, start/stop the network server, copy the listen URL, and watch listener count.

**Job to be done:** *"I want to run a live audio broadcast from my desktop with zero friction — load a sound, hit play, share a link, and see that people are listening."*

**Core screens / panels (single window):**
1. Top command bar — brand, master volume, live server status.
2. Sound Library — add sound, list of sound rows, per-row play/stop, volume, remove, master volume.
3. Network Server — status, listen URL + copy, listener count, start/stop, port, bind address.

**Design goal:** make the desktop app feel like the same product as the Red Noir web listener — an industrial broadcast console, not a generic settings dialog. Same palette, same mono telemetry language, same rose accent discipline. No new features; only re-skin and re-compose existing controls.

## Visual direction — "Red Noir Broadcast Console"

A dark hardware console: near-black surfaces, hairline zinc structure, one hot rose accent used like a signal lamp, monospaced telemetry, uppercase micro-labels, and a faint red scanline. Crisp and technical, never soft or playful.

- **One accent:** rose-600 is reserved for primary actions, live/playing signal, focus, and active state. Do not tint whole panels red.
- **Hairlines over shadows:** structure comes from 1px borders and spacing, not heavy elevation.
- **Square-ish geometry:** 2–4px radii. Avoid pill-shaped cards and 16–24px rounded panels.
- **Telemetry typography:** numbers, percentages, URLs, ports and status codes are mono, uppercase labels are tracked out.
- **Signal language:** a pulsing rose dot means live/playing; amber means starting/warning; zinc means idle; deep rose means error.
- **Quiet motion:** 150–250ms transitions, a slow scanline, pulse on live indicators. Respect reduced motion.

## Color tokens

Use these exact values. Names mirror `src/gui/theme.rs` and `src/network/static/style.css`.

| Token | Hex | Role |
|---|---|---|
| noir-black | `#0A0A0A` | Window / page background |
| noir-panel | `#111111` | Primary panel surface |
| noir-zinc-950 | `#09090B` | Deepest inset (wells, meters) |
| noir-zinc-900 | `#18181B` | Elevated surface / row background |
| noir-zinc-800 | `#27272A` | Default border / divider |
| noir-zinc-700 | `#3F3F46` | Strong border / track |
| noir-zinc-600 | `#52525B` | Footer copy |
| noir-zinc-500 | `#71717A` | Muted / label text |
| noir-zinc-400 | `#A1A1AA` | Secondary text |
| noir-zinc-300 | `#D4D4D8` | Body emphasis |
| rose-400 | `#FB7185` | Error / clipping accent |
| rose-500 | `#F43F5E` | Connected / playing accent |
| rose-600 | `#E11D48` | Primary brand accent |
| rose-700 | `#BE123C` | Primary hover / pressed |
| rose-950 | `#4C0519` | Meter base / tinted inset |
| amber-400 | `#FBBF24` | Starting / buffer warning |
| white | `#FFFFFF` | Primary text |
| white-5 | `rgba(255,255,255,.05)` | Hairline highlight |
| white-10 | `rgba(255,255,255,.10)` | Border on dark |
| white-20 | `rgba(255,255,255,.20)` | Peak marker / strong hairline |

**Semantic mapping:** idle `zinc-500`; live/playing `rose-500`; primary action `rose-600`; hover `rose-700`; starting `amber-400`; error `rose-400` on `rose-950` tint; disabled `zinc-600` at 50% opacity.

## Typography

- **Sans:** Archivo (400/500/700/900), fallback system sans. Used for titles, names, body.
- **Mono:** JetBrains Mono (400/700), fallback system mono. Used for all numbers, %, URLs, ports, sample rates, status telemetry, keyboard hints.
- Display / brand title: 26–32px, weight 900, uppercase, tracking `-0.01em`.
- Panel title: 16–18px, weight 900, uppercase, tracking `0.04em`.
- Micro-label: 10–11px, uppercase, tracking `0.12em`, `zinc-500`.
- Body: 13–14px, `zinc-300` / `white`.
- Mono stat: 11–13px, tabular.
- In egui: map to proportional and monospace families; use `FontId` sizes from this scale.

## Spacing, radius, structure

- Scale: 4 / 8 / 12 / 16 / 24 / 32.
- Panel padding: 24 (compact rows 12–16).
- Panel gap: 24; row gap: 8–10.
- Radius: buttons/inputs 2; panels 4; small chips 2. Nothing above 6.
- Hairline: 1px `zinc-800`; emphasis 1px `zinc-700`.
- Window: 1100×750; content max 1152; min 900×600. Do not introduce a responsive mobile layout — this is a desktop window, but content must not clip at minimum size.
- Top command bar: 64px, full width, bottom hairline, brand left, master volume center, status right.
- Main: two columns — library flexible (~62%) and server fixed 320–340px, 24px gap, 24px outer padding.
- Optional bottom status strip: 28px, mono, sample rate / buffer / version.

## Components

**Top command bar**
- Brand lockup: wordmark `SoundShared` 20px 900 with a small rose square/signal mark; subtitle `BROADCAST CONSOLE` in 10px uppercase mono `zinc-500`.
- Master volume: label `MASTER` 10px mono uppercase, 120–150px slider, mono `80%` readout. Use a square thumb and 2px track.
- Live status: dot + `SERVER STOPPED` / `LIVE` in 10–11px mono; live uses rose-500 with a slow pulse.

**Panel frame**
- `noir-panel` fill, 1px `zinc-800` border, 4px radius, 24px padding.
- Header row: uppercase 900 title + mono index/code (e.g. `01 / LIBRARY`) left; action right; 1px divider below.
- Prefer section codes and hairline rules over large rounded cards.

**Primary button**
- Rose-600 fill, white label, 2px radius, 36px height, 16px horizontal padding, optional leading glyph.
- Hover rose-700; pressed darker; focus 1px rose-400 ring.
- Label in 11px uppercase tracking `0.08em` when it reads as a command (e.g. `ADD SOUND`, `START SERVER`).

**Ghost / secondary button**
- Transparent or `zinc-900` fill, 1px `zinc-800` border, `zinc-300` label, 2px radius.
- Hover: `zinc-800` fill + `zinc-700` border.

**Danger / stop button**
- Transparent fill, 1px `rose-950`/`rose-700` border, rose-400 label; hover `rose-950` fill. Do not use a solid second red that competes with primary.

**Status indicator**
- 7–8px square or dot + 10–11px uppercase mono label. Live pulses; idle is flat zinc.
- Optional condensed status pill: 1px border, `zinc-950` fill, 24px height, 2px radius.

**Sound row**
- 56–64px tall, `zinc-900` fill, 1px `zinc-800` border, 2–4px radius.
- Left: 32px square transport button (rose-600 play / rose-outline stop).
- Middle: sound name 13–14px 500 white; secondary mono line for format/duration/path (`48 kHz · 00:42`).
- Right: small mono volume slider + `80%`, then a ghost remove action (trash or `×`).
- Playing state: 2px rose-500 left edge / rose dot, row border rose-600 at 40%, subtle rose-950 tint. No full red fill.
- Empty state: centered mono `NO SIGNAL` label, muted helper line, and a ghost `ADD SOUND` action; not a large empty card.

**Server panel**
- Header `02 / NETWORK`.
- Status block: large mono state (`STOPPED`, `LIVE`, `STARTING`), dot, and a one-line description.
- Listen URL: inset `zinc-950` field, 1px border, mono URL, copy button. On copy show rose check + `COPIED`.
- Listeners: big mono count (24–28px) + `LISTENERS` micro-label, plus a row of small square/dot indicators (filled rose up to count, max 8, then `+N`).
- Transport: full-width primary `START SERVER` or outlined danger `STOP SERVER`.
- Config: `PORT` and `BIND ADDRESS` micro-labels above compact square inputs; mono values; hairline divider above.
- Show a small inline mono note when stopped: `SERVER OFFLINE — START TO BROADCAST`.

**Meter / telemetry accents (optional, decorative only)**
- Use a 20-cell buffer strip or 12–16px signal bars only where they reinforce live state; never fabricate data that the app does not have. Prefer listener count and play state as the real telemetry.

## Motion

- `--transition-fast: 150ms ease`, `--transition-normal: 250ms ease`.
- Live dot: 2s pulse (scale/opacity), paused when not live.
- Background: extremely subtle red scanline drift, ≤3% opacity, optional.
- Row hover: border brightens to `zinc-700` in 150ms; no movement or scaling.
- Respect `prefers-reduced-motion`: disable pulse/scanline, keep instant state changes.

## Content and tone

- Terse, technical, lowercase-free labels in caps for chrome: `SOUND LIBRARY`, `NETWORK SERVER`, `MASTER`, `LISTENERS`, `PORT`, `BIND ADDRESS`.
- Sound names keep original case and may truncate with ellipsis + tooltip.
- Numbers always mono: `80%`, `8080`, `1 listener`, `3 listeners`.
- Do not add marketing copy, nav, accounts, or settings that do not exist in the app.

## Implementation constraints (egui)

- Keep the existing window size, all controls, and their behavior exactly.
- Implement with egui frames, strokes, `RichText`, custom-painted dots/rules, and the existing `theme.rs` token constants (update tokens to the values above).
- Do not add images, icon fonts, or web-only effects that egui cannot reproduce; monospace glyphs and painted shapes are fine.
- Preserve keyboard accessibility and readable contrast (minimum 4.5:1 for body text).
