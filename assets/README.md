# assets/

## Application icon

The executable icon is the red "Red Noir" equaliser mark: a squircle filled with
the brand gradient (`ROSE_500` → `ROSE_700` from `src/gui/theme.rs`) carrying
near-black bars (`Palette::APP`). No lettering, so the same mark works for the
working title *SoundShared* and the published name *Bardwave*.

### How it reaches the binary

```
assets/icon.svg            vector source (documentation / future edits)
assets/icon.ico  ──┐
assets/icon.rc   ──┴─► build.rs (rc.exe) ──► target/<profile>/soundshared.res
                                              │
                                              └─► linked into soundshared.exe
                                                    (Explorer, taskbar, Alt-Tab)
assets/icon-64.rgba ─────► src/gui/mod.rs `window_icon()` (title bar / taskbar
                           while the app is running)
```

`build.rs` finds `rc.exe` by itself (`RC`/`RC_EXE`, then `PATH`, then the newest
installed Windows Kits entry, then a `vswhere` lookup) and needs no extra crate.
Without a Windows SDK it prints a warning and the binary is simply built without
an icon — the build never fails because of it. `assets/icon.rc` deliberately has
no `#include` lines so `rc.exe` does not need an SDK include path either.

### Regenerating

```bash
node assets/tools/gen-icon.mjs          # rewrites every file listed below
node assets/tools/gen-icon.mjs --ascii  # also prints the small cuts as text
node assets/tools/verify-exe-icon.mjs   # after cargo build: proves the exe has them
```

| File | Purpose |
| --- | --- |
| `icon.svg` | Vector source, 256×256 grid |
| `icon.ico` | Windows icon, 9 resolutions (16–256), all plain 32bpp bitmaps |
| `icons/icon-<n>.png` | PNG ladder: 16, 20, 24, 32, 40, 48, 64, 128, 256, 512 |
| `icon.png` | 256px PNG for docs / store listings |
| `icon-64.rgba` | Straight-alpha RGBA for `egui::IconData` (no PNG decoder at runtime) |
| `icon-preview.png` | Contact sheet: every cut on a dark and a light desktop |
| `icon.rc` | Resource script — icon group + version info |

All of it is generated from the numbers in `assets/tools/gen-icon.mjs`, so the vector
file, the PNG ladder, the `.ico` and the runtime RGBA cannot drift apart.

### Sizing rules baked into the generator

* **16–64px** are snapped to whole pixels per size (bar widths, gaps and heights
  come from a table). Proportional bars at 16px are ~1.2px wide with 1.2px gaps,
  which antialiasing smears into a single grey blob.
* **16–24px** use three bars, **32px and up** use five: five bars inside a 16px
  square stop being distinguishable.
* The inner hairline is only drawn at **48px and above**, where it has room.
* `.ico` entries are uncompressed bitmaps rather than PNG-compressed, which the
  Windows shell accepts but GDI+/Java-style decoders do not.
