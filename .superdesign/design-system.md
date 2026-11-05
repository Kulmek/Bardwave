# SoundShared — Desktop GUI Design System

> **Code truth.** Every number below is taken from `src/gui/theme.rs` (`Metrics`,
> `Palette`), `src/gui/mod.rs` (window + shell geometry), `src/gui/top_bar.rs`,
> `src/gui/sound_list.rs` and `src/gui/server_panel.rs`. When this document and the
> code disagree, the code wins — and the disagreement is a bug in this file.

## Product context

**Product:** SoundShared is a local audio streaming server. One machine loads and mixes sound files; listeners open a browser and hear the live mix over WebSocket.

**Target surface:** the native desktop control app (egui), a single window at **1280×800** (minimum **960×620**). It is the operator's control room: load sounds, play/stop them, set per-sound and master volume, start/stop the network server, copy the listen URL, and watch listener count.

**Job to be done:** *"I want to run a live audio broadcast from my desktop with zero friction — load a sound, hit play, share a link, and see that people are listening."*

**Core screens / panels (single window):**
1. Top command bar — brand, master volume + `PANIC`, live server status.
2. Sound Library — add sound, group rail, list of sound rows, per-row play/stop, volume, repeat, remove, master output strip.
3. Network Server — status, listen URL + copy, listener occupancy, start/stop, editable port, bind address.
4. Bottom status strip — version, **local output readout**, format/track/group telemetry.

**Design goal:** make the desktop app feel like the same product as the Red Noir web listener — an industrial broadcast console, not a generic settings dialog. Same palette, same mono telemetry language, same rose accent discipline. No new features; only re-skin and re-compose existing controls.

## Visual direction — "Red Noir Broadcast Console"

A dark hardware console: near-black surfaces, hairline zinc structure, one hot rose accent used like a signal lamp, monospaced telemetry, uppercase micro-labels, and a faint red scanline. Crisp and technical, never soft or playful.

- **One accent:** rose-600 is reserved for primary actions, live/playing signal, focus, and active state. Do not tint whole panels red.
- **Hairlines over shadows:** structure comes from 1px borders and spacing, not heavy elevation.
- **Square-ish geometry:** 2–4px radii. Avoid pill-shaped cards and 16–24px rounded panels.
- **Telemetry typography:** numbers, percentages, URLs, ports and status codes are mono, uppercase labels are tracked out.
- **Signal language:** a pulsing rose dot means live/playing; amber means starting/warning; zinc means idle; deep rose means error.
- **Quiet motion:** pulse on live indicators, 150–250ms transitions, slow scanline. Respect reduced motion.

## Color tokens

Use these exact values. Names mirror `src/gui/theme.rs` (`Palette`) and `src/network/static/style.css`.

| Token | Hex | Role |
|---|---|---|
| noir-black | `#0A0A0A` | Window / page background (`Palette::APP`) |
| noir-panel | `#111111` | Primary panel surface (`Palette::PANEL`) |
| noir-zinc-950 | `#09090B` | Deepest inset (wells, meters, inputs) (`Palette::WELL`) |
| noir-zinc-900 | `#18181B` | Elevated surface / row background (`Palette::ROW`) |
| noir-zinc-850 | `#1F1F23` | Row background on hover (`Palette::ROW_HOVER`) |
| noir-zinc-800 | `#27272A` | Default border / divider (`Palette::BORDER`) |
| noir-zinc-700 | `#3F3F46` | Strong border / track (`Palette::BORDER_STRONG`) |
| noir-zinc-600 | `#52525B` | Footer copy / disabled text |
| noir-zinc-500 | `#71717A` | Muted / micro-label text |
| noir-zinc-400 | `#A1A1AA` | Secondary text |
| noir-zinc-300 | `#D4D4D8` | Body emphasis |
| rose-400 | `#FB7185` | Error / clipping / `SAVED` accent (`Palette::ROSE_400`) |
| rose-500 | `#F43F5E` | Connected / playing accent (`Palette::ROSE_500`) |
| rose-600 | `#E11D48` | Primary brand accent (`Palette::ROSE_600`) |
| rose-700 | `#BE123C` | Primary hover / pressed (`Palette::ROSE_700`) |
| rose-950 | `#4C0519` | Meter base / tinted inset (`Palette::ROSE_950`) |
| amber-400 | `#FBBF24` | Starting / buffer warning (`Palette::AMBER_400`) |
| white | `#FFFFFF` | Primary text |

**Semantic mapping:** idle `zinc-500`; live/playing `rose-500`; primary action `rose-600`; hover `rose-700`; starting `amber-400`; error `rose-400`; disabled `zinc-600`; well fill `zinc-950`.

## Typography

- **Text:** egui's default proportional family (`ui_font(size)`). No bundled webfonts — do not specify Archivo/JetBrains Mono as a build dependency.
- **Telemetry:** egui's monospace family (`mono_font(size)` / `glyph_font(size)`).
- Wordmark `SoundShared`: 18px 900 proportional (`wordmark()`), tracking -0.2px; subtitle `BROADCAST CONSOLE` 9px mono uppercase `zinc-500`.
- Panel title: 18px 900 uppercase, tracking 0.4px (`panel_title()`).
- Section code (`01 / Library`): 10px mono uppercase, tracked, `rose-500` (`micro_label_accent`).
- **Micro-label:** 10px mono uppercase, letter-spacing 1.2px, `zinc-500` (`micro_label`).
- **Micro-label tiny:** 9px mono uppercase, letter-spacing 0.8–1.0px — used for hints, `SAVED`, `COPIED TO CLIPBOARD`, status-strip text.
- Big telemetry: server state 24px mono strong; listener count 30px mono; empty-state `NO SIGNAL` 22px mono `zinc-700`.
- Body: 13–14px `zinc-300` / `white`; mono stat 11px.

## Spacing, radius, structure

- Scale (`Metrics`): 4 / 8 / 12 / 16 / 24 / 32.
- Panel inner padding 24; outer window padding 24; gap between the two main columns 24; row gap 10.
- Radius (`Metrics`): inputs/buttons/rows 2; panels 4. Nothing above 4.
- Hairline: 1px `zinc-800`; emphasis 1px `zinc-700`. A panel strokes its hairline *outside* its rect, so a layout rect is the fill box and is 1px smaller than the painted border box.
- **Window: 1280×800; minimum 960×620. No max-content clamp.** The shell fills the window: 24px outer padding, two columns with a 24px gap, right column fixed at **340px** (`RIGHT_COL_W`), library takes the rest.
- Top command bar: **64px** (`TOPBAR_H`), full width, bottom hairline.
- Bottom status strip: **28px** (`STATUS_H`), always present, full width, top hairline.
- Sound row: **64px** (`ROW_H`); group rail entry 30px.

## Components

**Top command bar** (64px)
- Brand lockup: 18px rose-600 rounded square with a white wave glyph; wordmark `SoundShared` 20px 900; subtitle `BROADCAST CONSOLE` 9px mono `zinc-500`.
- Vertical hairline dividers (1×32px, `zinc-800`) separate the groups.
- Master volume: micro-label `MASTER`, 150px slider (`MASTER_SLIDER_W`), mono `80%` readout in white.
- Panic: ghost icon button 26px with a stop glyph + micro-label `PANIC`. Stops every playing sound; quiet by design, not a second red button.
- Live status (right-aligned, two lines): 7px dot (pulses when live) + mono 11px uppercase tracked headline; under it a 9px mono tracked subline. States: `LIVE` / `SERVER RUNNING · :8080` (rose-500) · `STARTING` / `BINDING LISTENER` (amber-400) · `ERROR` / `SERVER FAILED — SEE PANEL` (rose-400) · `OFF AIR` / `SERVER STOPPED` (zinc-500).

**Panel frame**
- `noir-panel` fill, 1px `zinc-800` border, 4px radius, 24px padding.
- Header row: 10px rose section code above a 16px 900 uppercase title (`01 / Sound Library`, `02 / Network`).
- Prefer section codes and hairline rules over large rounded cards.

**Primary button** (`ConsoleButton::primary`)
- Rose-600 fill, white label, 2px radius, **36px** height (`BTN_H`; the network transport uses 40px), 16px horizontal padding, optional leading glyph.
- Hover rose-700; focus 1px rose-400 ring drawn just outside the border (`ConsoleButton`). Label 11px uppercase tracking `0.08em` for commands (`ADD SOUND`, `START SERVER`).
- Keyboard: a click focuses the button, `Tab`/`Shift+Tab` move focus through the focusable widgets, and `Space`/`Enter` on a focused button activate it. egui delivers that keystroke as `Response::fake_primary_click`, which `ConsoleButton`/`IconButton` fold into `clicked()` so every call site is unchanged.

**Ghost / secondary button** (`ConsoleButton::ghost`)
- Transparent or `zinc-900` fill, 1px `zinc-800` border, `zinc-300` label, 2px radius, 28–36px height. Hover: `zinc-800` fill + `zinc-700` border.
- Icon variant: square, 26–30px, centered glyph, tooltip carries the name.

**Danger / stop button**
- Transparent fill, 1px `rose-700` border, rose-400 label; hover `rose-950` fill. Never a solid second red competing with the primary action.

**Status indicator**
- 7–8px square or dot + 10–11px uppercase mono label. Live pulses; idle flat zinc.

**Sound row** (64px)
- `zinc-900` fill, 1px `zinc-800` border, 2px radius, 10px gap between rows.
- Left: 32px square transport button (`TRANSPORT`; rose-600 play / outlined stop), then the repeat toggle — an outlined square that fills rose-950/rose-500 when the sound restarts itself.
- Middle: sound name 13–14px 500 white (playing: rose-400) with a pulsing 7px rose dot in front; secondary mono line `48 kHz / 00:42 / WAV` in `zinc-500`.
- Right (right-to-left): ghost remove `×` 18px (`zinc-500`, rose-400 on hover), `80%` mono readout, 80px volume slider (`ROW_SLIDER_W`), then the group tag chip (outlined `zinc-800`, 10px mono uppercase `zinc-500`).
- Playing state: 2px rose-500 left edge, row border rose-600 at ~43%, fill rose-950 at ~27%. No full red fill.

**Empty library state** *(centred inside the library panel's list area, replacing the rows)*
- Line 1: `NO SIGNAL` 22px mono `zinc-700`.
- Line 2: context descriptor 11px mono uppercase `zinc-400` — `NO SOUNDS YET` (all), `NOTHING UNGROUPED`, or `<GROUP NAME> IS EMPTY`.
- Line 3: micro-label hint — `ADD A WAV / OGG / MP3 TO START A BOARD`, or `ADD A SOUND TO ATTACH IT TO THIS GROUP` when a group is selected.
- Action: 150px primary `ADD SOUND` (plus glyph) that opens the file dialog; when a filtered view is empty, a 150px ghost `SHOW ALL` sits under it. Never a large empty card.

**Empty listeners state** *(inside `02 / NETWORK`)*
- The empty state is the real telemetry, never a fabricated label: mono count `00` at 30px white, all eight 8px occupancy cells hollow (`zinc-800`), and `PEAK 0` on the right.
- The big-state subline carries the words: `BROADCASTING TO 0 LISTENERS` while the server is live, `SERVER STOPPED` while it is off air (`LIVE` / `STARTING` / `ERROR` headline in the big state above).

**Server panel** (`02 / Network`)
- Big state: 8px dot + 24px mono uppercase headline (`LIVE`, `STARTING`, `ERROR`, `OFF AIR`) + 10px micro-label subline.
- Listen URL: inset `zinc-950` field 40px tall, 1px border, mono URL, 30px ghost copy button. On copy: `COPIED TO CLIPBOARD` in 9px rose-400, tracked, for ~1.5s.
- Listeners: 30px mono count (`05`), then eight 8px occupancy cells (filled rose-500 up to the count, hollow `zinc-800` after), `PEAK N` micro-label right-aligned.
- Transport: primary `START SERVER` / `RETRY SERVER` / `STOP SERVER` (40px) beside a 112px ghost `COPY LINK`.
- Hairline divider above the configuration block.

**Configuration fields — editable wells** *(the two inputs under `CONFIGURATION`)*
- Geometry: `PORT` well 80px wide, `BIND ADDRESS` well 120px wide, both **36px** tall, 12px apart, 12px inner side padding, 12px mono text — the pair fills the 292px panel body.
- Style: `zinc-950` fill, 1px `zinc-800` border, 2px radius; text white when editable, `zinc-600` when disabled. Focus ring 1px rose-600.
- These are **real text inputs**, not painted telemetry: they accept typing while the server is stopped and are non-interactive (greyed) while `LIVE`/`STARTING`.
- Validation is inline and live: invalid text never commits, and a 9px rose-400 tracked line appears under the row — `PORT MUST BE 1–65535` or `BIND MUST BE AN IP ADDRESS`. Only plain digits (`1..=65535`) and IP literals (`0.0.0.0`, `127.0.0.1`, `::`, `::1`) are accepted.
- Under the row: micro-label hint `EDITABLE WHILE STOPPED` / `STOP THE SERVER TO EDIT`, and on the right `SAVED` in 9px rose-400 tracked for 1.5s after a valid edit lands.
- Max lengths: port 5 chars, bind address 45 chars (a full IPv6 literal); longer text never fits, so it is not accepted.
- Below the divider: `WS / 48 KHZ / STEREO` mono telemetry line.

**Bottom status strip** (28px, three zones)
- Left: `SoundShared v<version>` 9px mono `zinc-600` tracked.
- Centre-left: **local output readout** — 6px dot + 9px mono uppercase text, tracked 1.0px, truncated rather than wrapped, max 320px.
  - `LOCAL · <DEVICE>` — dot rose-500 pulsing, text `zinc-400` (`LOCAL OUTPUT RUNNING`).
  - `LOCAL OFF — HEADLESS (NETWORK ONLY)` — dot and text amber-400.
  - `LOCAL FAILED — <REASON>` — dot rose-700, text rose-400.
  - `LOCAL STOPPED` — dot zinc-500, text `zinc-600`.
  - This readout exists because the web listeners are fed from the mixer in every one of these states: without it, a silent desktop looks identical to a working one.
- Right: `48 kHz / 16-bit / stereo · N tracks · M groups` 9px mono `zinc-600` tracked.

**Meter / telemetry accents (optional, decorative only)**
- Use a 20-cell buffer strip or 12–16px signal bars only where they reinforce live state; never fabricate data the app does not have. Listener count and play state are the real telemetry.

## Motion

- Live dot: 2s pulse (opacity breath), paused when not live.
- Background: extremely subtle red scanline drift, ≤3% opacity (`paint_backdrop`, 1px rose-600 at alpha 7/255 ≈ 2.7%).
- Row hover: border brightens to `zinc-700`; no movement or scaling.
- Transient confirmations (`SAVED`, `COPIED`) hold ~1.5s then disappear.
- **Reduced motion** (implemented, not optional): the pulse and the scanline stop; every state change stays instant. egui 0.30 exposes no OS signal for this, so the operator states it — either `config.toml` → `[gui] reduce_motion = true`, or the per-session override `SOUNDSHARED_REDUCED_MOTION=1` (wins over the file). Resolution lives in `src/gui/motion.rs`; unrecognised env values fall back to the config, so a typo can never change the animation.
- `signal_dot` keeps drawing the dot when the pulse is off — the state it reports is never hidden, only the breath is dropped.

## Content and tone

- Terse, technical, uppercase chrome labels: `SOUND LIBRARY`, `NETWORK SERVER`, `MASTER`, `LISTENERS`, `PORT`, `BIND ADDRESS`, `LOCAL`, `PANIC`.
- Sound names keep original case and may truncate with ellipsis + tooltip.
- Numbers always mono: `80%`, `8080`, `05`, `PEAK 3`, `1 listener`, `3 listeners`.
- Do not add marketing copy, nav, accounts, or settings that do not exist in the app.

## Implementation constraints (egui)

- Keep the existing window size, all controls, and their behaviour exactly.
- Implement with egui frames, strokes, `RichText`, custom-painted dots/rules, and the existing `theme.rs` token constants.
- Do not add images, icon fonts, or web-only effects egui cannot reproduce; monospace glyphs and painted shapes are fine.
- Preserve keyboard accessibility and readable contrast (minimum 4.5:1 for body text).
