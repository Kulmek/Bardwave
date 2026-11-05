//! Custom-painted console widgets for the Red Noir broadcast console.
//!
//! egui's default widgets are soft and rounded; the console needs hairline
//! structure, square geometry, mono telemetry and a single rose accent. These
//! helpers paint that look directly and report interaction through `Response`.

use std::borrow::Cow;

use egui::{
    Align2, Color32, CursorIcon, FontId, Rect, Response, Rounding, Sense, Stroke, Ui, Vec2,
};

use crate::gui::theme::{alpha, mono_font, Metrics, Palette};

/// An icon glyph drawn with the monospace font (no icon font dependency).
pub type Glyph = &'static str;

pub mod glyph {
    use super::Glyph;

    /// ▶ — play.
    pub const PLAY: Glyph = "\u{25B6}";
    /// ■ — stop.
    pub const STOP: Glyph = "\u{25A0}";
    /// ✕ — remove / close.
    pub const CROSS: Glyph = "\u{2715}";
    /// ✎ — edit / rename.
    pub const EDIT: Glyph = "\u{270E}";
    /// ⧉ — copy.
    pub const COPY: Glyph = "\u{29C9}";
    /// + — add.
    pub const PLUS: Glyph = "+";
    /// ✓ — confirmed.
    pub const CHECK: Glyph = "\u{2713}";
    /// ⟳ — refresh / restart.
    pub const REFRESH: Glyph = "\u{27F3}";
    /// ⟲ — repeat / loop playback.
    pub const LOOP: Glyph = "\u{27F2}";
    /// ≈ — waveform-ish mark used in the brand lockup.
    pub const WAVE: Glyph = "\u{224B}";
}

/// Visual weight of a button.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BtnKind {
    /// Solid rose-600 fill, white label. Primary action.
    Primary,
    /// Transparent fill, hairline border, zinc-300 label.
    Ghost,
    /// Transparent fill, rose-700 hairline, rose-400 label.
    Danger,
    /// Solid rose-950 fill, rose-400 label (used while a sound is playing).
    Armed,
}

/// A console button with optional leading glyph, square 2px geometry.
pub struct ConsoleButton<'a> {
    text: Cow<'a, str>,
    glyph: Option<Glyph>,
    kind: BtnKind,
    size: Vec2,
    enabled: bool,
    tooltip: Option<Cow<'a, str>>,
    id_salt: Option<&'a str>,
    font_size: f32,
    tracking: f32,
}

impl<'a> ConsoleButton<'a> {
    pub fn new(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            glyph: None,
            kind: BtnKind::Ghost,
            size: Vec2::new(0.0, Metrics::BTN_H),
            enabled: true,
            tooltip: None,
            id_salt: None,
            font_size: 11.0,
            tracking: 0.9,
        }
    }

    pub fn primary(text: impl Into<Cow<'a, str>>) -> Self {
        Self::new(text).kind(BtnKind::Primary)
    }

    pub fn ghost(text: impl Into<Cow<'a, str>>) -> Self {
        Self::new(text).kind(BtnKind::Ghost)
    }

    pub fn danger(text: impl Into<Cow<'a, str>>) -> Self {
        Self::new(text).kind(BtnKind::Danger)
    }

    pub fn kind(mut self, kind: BtnKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn glyph(mut self, glyph: Glyph) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// Explicit size. A zero width means "fill the available width".
    pub fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.size.y = height;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.size.x = width;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<Cow<'a, str>>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn id_salt(mut self, salt: &'a str) -> Self {
        self.id_salt = Some(salt);
        self
    }

    pub fn small(mut self) -> Self {
        self.size.y = Metrics::BTN_H_SM;
        self.font_size = 10.0;
        self
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    pub fn no_tracking(mut self) -> Self {
        self.tracking = 0.0;
        self
    }

    /// Paint the button and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        // A stable id makes hover/tooltip state survive frames even though the
        // button paints itself instead of using `ui.button`.
        let salt = match self.id_salt {
            Some(salt) => salt.to_string(),
            None => self.text.to_string(),
        };
        ui.push_id(("console_btn", salt), |ui| self.show_inner(ui))
            .inner
    }

    fn show_inner(self, ui: &mut Ui) -> Response {
        let width = if self.size.x > 0.0 {
            self.size.x
        } else {
            ui.available_width().max(40.0)
        };
        let size = Vec2::new(width, self.size.y);

        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, mut resp) = ui.allocate_exact_size(size, sense);
        if self.enabled {
            resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        }

        let hovered = self.enabled && resp.hovered();
        let pressed = self.enabled && resp.is_pointer_button_down_on();

        // Keyboard accessibility, both halves of it:
        //
        // * egui reports `Space`/`Enter` on a focused widget as
        //   `fake_primary_click`, so it is folded into `clicked` here and every
        //   caller keeps using `.clicked()`;
        // * clicking a focusable widget does not move focus by itself, so a
        //   click focuses it — which is what makes the ring appear and the
        //   follow-up keystroke work.
        if resp.clicked() {
            resp.request_focus();
        }
        if resp.fake_primary_click {
            resp.clicked = true;
        }
        let focused = self.enabled && resp.has_focus();

        let (fill, border, text_color, glyph_color) = match self.kind {
            BtnKind::Primary => {
                // Pressed and hovered both darken to rose-700.
                let fill = if pressed || hovered {
                    Palette::ROSE_700
                } else {
                    Palette::ROSE_600
                };
                (fill, Palette::ROSE_600, Palette::WHITE, Palette::WHITE)
            }
            BtnKind::Ghost => {
                let fill = if pressed {
                    Palette::BORDER
                } else if hovered {
                    Palette::ROW
                } else {
                    Palette::TRANSPARENT
                };
                let border = if hovered {
                    Palette::BORDER_STRONG
                } else {
                    Palette::BORDER
                };
                (fill, border, Palette::ZINC_300, Palette::ZINC_400)
            }
            BtnKind::Danger => {
                let fill = if pressed || hovered {
                    alpha(Palette::ROSE_950, 180)
                } else {
                    Palette::TRANSPARENT
                };
                (
                    fill,
                    Palette::ROSE_700,
                    Palette::ROSE_400,
                    Palette::ROSE_400,
                )
            }
            BtnKind::Armed => {
                let fill = if hovered {
                    alpha(Palette::ROSE_950, 230)
                } else {
                    Palette::ROSE_950
                };
                let border = if hovered {
                    Palette::ROSE_500
                } else {
                    alpha(Palette::ROSE_500, 150)
                };
                (fill, border, Palette::ROSE_400, Palette::ROSE_400)
            }
        };

        let fill = if self.enabled {
            fill
        } else {
            Palette::TRANSPARENT
        };
        let text_color = if self.enabled {
            text_color
        } else {
            alpha(Palette::ZINC_600, 140)
        };

        let painter = ui.painter();
        if fill != Palette::TRANSPARENT {
            painter.rect_filled(rect, Rounding::same(Metrics::R_BTN), fill);
        }
        painter.rect_stroke(
            rect,
            Rounding::same(Metrics::R_BTN),
            Stroke::new(
                1.0_f32,
                if self.enabled {
                    border
                } else {
                    Palette::BORDER
                },
            ),
        );

        // 1px rose-400 focus ring, drawn just outside the border so it never
        // hides the button's own edge.
        if focused {
            painter.rect_stroke(
                rect.expand(1.0),
                Rounding::same(Metrics::R_BTN + 1.0),
                Stroke::new(1.0_f32, Palette::ROSE_400),
            );
        }

        let font = mono_font(self.font_size);
        let label = self.text.to_uppercase();

        // Measure the full label block, then centre it.
        let glyph_galley = self
            .glyph
            .map(|g| painter.layout_no_wrap(g.to_owned(), font.clone(), glyph_color));
        let text_galley = painter.layout_no_wrap(label.clone(), font.clone(), text_color);

        let glyph_w = glyph_galley
            .as_ref()
            .map(|g| g.size().x + 6.0)
            .unwrap_or(0.0);
        let total_w = glyph_w + text_galley.size().x;
        let mut cursor_x = rect.center().x - total_w / 2.0;

        if let Some(galley) = glyph_galley {
            let pos = egui::pos2(cursor_x, rect.center().y - galley.size().y / 2.0);
            painter.galley(pos, galley, glyph_color);
            cursor_x += glyph_w;
        }
        let pos = egui::pos2(cursor_x, rect.center().y - text_galley.size().y / 2.0);
        painter.galley(pos, text_galley, text_color);

        if let Some(tooltip) = self.tooltip {
            resp = resp.on_hover_text(tooltip.as_ref());
        }
        resp
    }
}

/// Square icon button used for transport controls and row actions.
pub struct IconButton<'a> {
    glyph: Glyph,
    kind: BtnKind,
    size: f32,
    tooltip: Option<Cow<'a, str>>,
    id_salt: Option<&'a str>,
    font_size: f32,
}

impl<'a> IconButton<'a> {
    pub fn new(glyph: Glyph) -> Self {
        Self {
            glyph,
            kind: BtnKind::Ghost,
            size: Metrics::TRANSPORT,
            tooltip: None,
            id_salt: None,
            font_size: 12.0,
        }
    }

    pub fn kind(mut self, kind: BtnKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<Cow<'a, str>>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn id_salt(mut self, salt: &'a str) -> Self {
        self.id_salt = Some(salt);
        self
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let salt = match self.id_salt {
            Some(salt) => salt.to_string(),
            None => self.glyph.to_string(),
        };
        ui.push_id(("console_icon", salt), |ui| self.show_inner(ui))
            .inner
    }

    fn show_inner(self, ui: &mut Ui) -> Response {
        let size = Vec2::splat(self.size);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let mut resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        let hovered = resp.hovered();
        let pressed = resp.is_pointer_button_down_on();

        // Same keyboard contract as `ConsoleButton`: a click focuses the button,
        // and `Space`/`Enter` on the focused button counts as a click.
        if resp.clicked() {
            resp.request_focus();
        }
        if resp.fake_primary_click {
            resp.clicked = true;
        }
        let focused = resp.has_focus();

        let (fill, border, fg) = match self.kind {
            BtnKind::Primary => {
                let fill = if hovered || pressed {
                    Palette::ROSE_700
                } else {
                    Palette::ROSE_600
                };
                (fill, fill, Palette::WHITE)
            }
            BtnKind::Armed => {
                let fill = if hovered {
                    alpha(Palette::ROSE_950, 235)
                } else {
                    Palette::ROSE_950
                };
                let border = if hovered {
                    Palette::ROSE_500
                } else {
                    alpha(Palette::ROSE_500, 150)
                };
                (fill, border, Palette::ROSE_400)
            }
            BtnKind::Danger => {
                let fill = if hovered {
                    alpha(Palette::ROSE_950, 180)
                } else {
                    Palette::TRANSPARENT
                };
                (fill, Palette::ROSE_700, Palette::ROSE_400)
            }
            BtnKind::Ghost => {
                let fill = if hovered {
                    alpha(Palette::BORDER, 160)
                } else {
                    Palette::TRANSPARENT
                };
                (
                    fill,
                    if hovered {
                        alpha(Palette::BORDER_STRONG, 120)
                    } else {
                        Palette::TRANSPARENT
                    },
                    if hovered {
                        Palette::WHITE
                    } else {
                        Palette::ZINC_500
                    },
                )
            }
        };

        let painter = ui.painter();
        if fill != Palette::TRANSPARENT {
            painter.rect_filled(rect, Rounding::same(Metrics::R_SM), fill);
        }
        if border != Palette::TRANSPARENT {
            painter.rect_stroke(
                rect,
                Rounding::same(Metrics::R_SM),
                Stroke::new(1.0_f32, border),
            );
        }
        if focused {
            painter.rect_stroke(
                rect.expand(1.0),
                Rounding::same(Metrics::R_SM + 1.0),
                Stroke::new(1.0_f32, Palette::ROSE_400),
            );
        }
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            self.glyph,
            mono_font(self.font_size),
            fg,
        );

        match self.tooltip {
            Some(tooltip) => resp.on_hover_text(tooltip.as_ref()),
            None => resp,
        }
    }
}

/// Horizontal console slider: 2px track, filled rose portion, square 9px thumb.
///
/// Returns the response; `changed()` tells whether the value was modified.
pub fn level_slider(ui: &mut Ui, value: &mut f32, width: f32, id_salt: &str) -> Response {
    let height = 18.0;
    let (rect, mut resp) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());

    let min_y = rect.center().y - 1.0;
    let track = Rect::from_min_size(egui::pos2(rect.left(), min_y), Vec2::new(rect.width(), 2.0));

    let mut value_changed = false;
    if let Some(pointer) = resp.interact_pointer_pos() {
        if resp.is_pointer_button_down_on() || resp.dragged() {
            let t = ((pointer.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
            if (t - *value).abs() > f32::EPSILON {
                *value = t;
                value_changed = true;
            }
        }
    }
    // Keyboard adjustment for accessibility.
    if resp.has_focus() {
        let step = if ui.input(|i| i.modifiers.shift) {
            0.1
        } else {
            0.02
        };
        let delta = ui.input(|i| {
            let mut d = 0.0;
            if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowUp) {
                d += step;
            }
            if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowDown) {
                d -= step;
            }
            d
        });
        if delta != 0.0 {
            *value = (*value + delta).clamp(0.0, 1.0);
            value_changed = true;
        }
    }
    if value_changed {
        resp.mark_changed();
    }
    resp = resp.on_hover_cursor(CursorIcon::ResizeHorizontal);
    resp.widget_info(|| {
        egui::WidgetInfo::slider(true, *value as f64, format!("{:.0}%", *value * 100.0))
    });

    let hovered = resp.hovered() || resp.dragged();
    let value = value.clamp(0.0, 1.0);

    let painter = ui.painter();
    painter.rect_filled(track, Rounding::ZERO, Palette::BORDER_STRONG);
    if value > 0.0 {
        let filled =
            Rect::from_min_size(track.min, Vec2::new(track.width() * value, track.height()));
        painter.rect_filled(filled, Rounding::ZERO, Palette::ROSE_500);
    }
    let thumb_x = track.left() + track.width() * value;
    let thumb_size = if hovered { 6.0 } else { 4.5 };
    let thumb = Rect::from_center_size(
        egui::pos2(thumb_x, track.center().y),
        Vec2::splat(thumb_size),
    );
    painter.rect_filled(thumb, Rounding::ZERO, Palette::ROSE_500);

    let _ = id_salt;
    resp
}

/// A segmented meter (e.g. the 20-cell master output strip).
pub fn segmented_meter(ui: &mut Ui, value: f32, cells: usize, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    if cells == 0 {
        return;
    }
    let gap = 3.0;
    let cell_w = ((rect.width() - gap * (cells as f32 - 1.0)) / cells as f32).max(1.0);
    let lit = (value.clamp(0.0, 1.0) * cells as f32).round() as usize;
    let painter = ui.painter();
    for i in 0..cells {
        let x = rect.left() + (cell_w + gap) * i as f32;
        let cell = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(cell_w, rect.height()));
        let color = if i < lit {
            Palette::ROSE_500
        } else {
            Palette::BORDER
        };
        painter.rect_filled(cell, Rounding::ZERO, color);
    }
}

/// Small inventory of a series of square cells (listener occupancy).
pub fn occupancy_cells(ui: &mut Ui, filled: usize, total: usize, cell: f32, gap: f32) {
    let width = total as f32 * cell + gap * (total.saturating_sub(1)) as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, cell), Sense::hover());
    let painter = ui.painter();
    for i in 0..total {
        let x = rect.left() + (cell + gap) * i as f32;
        let r = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::splat(cell));
        let color = if i < filled {
            Palette::ROSE_500
        } else {
            Palette::BORDER
        };
        painter.rect_filled(r, Rounding::ZERO, color);
    }
}

/// Signal dot. `pulse` animates the opacity in a slow 2s breath.
pub fn signal_dot(ui: &mut Ui, color: Color32, diameter: f32, pulse: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
    let color = if pulse {
        let t = ui.input(|i| i.time) as f32;
        let phase = ((t * std::f32::consts::PI).sin() + 1.0) * 0.5; // 0..1, 2s period
        let a = (120.0 + 135.0 * phase) as u8;
        alpha(color, a)
    } else {
        color
    };
    ui.painter()
        .circle_filled(rect.center(), diameter / 2.0, color);
    resp
}

/// Shorten a string to fit roughly `max_chars` characters, adding an ellipsis.
pub fn elide(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('\u{2026}');
    out
}

/// Format seconds as `MM:SS`.
pub fn format_duration(seconds: f32) -> String {
    let total = seconds.max(0.0).round() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Format a sample rate as `48 KHZ`.
pub fn format_sample_rate(hz: u32) -> String {
    if hz >= 1000 && hz.is_multiple_of(1000) {
        format!("{} kHz", hz / 1000)
    } else {
        format!("{:.1} kHz", hz as f32 / 1000.0)
    }
}

/// Uppercase file extension of a path, or `AUDIO` when unknown.
pub fn format_extension(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_uppercase())
        .unwrap_or_else(|| "AUDIO".to_string())
}

/// Monospace font used for painted glyphs at a given size.
pub fn glyph_font(size: f32) -> FontId {
    mono_font(size)
}
