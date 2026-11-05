//! SoundShared GUI — Red Noir broadcast console shell.
//!
//! Single-window operator console:
//!
//! ```text
//! ┌ top command bar (64px) ───────────────────────────────────┐
//! │ brand · master volume · panic · live status               │
//! ├───────────────────────────────────────────────────────────┤
//! │ 01/SOUND LIBRARY  (flex)        │ 02/NETWORK (340px)      │
//! │ groups rail + sound rows        │ server controls         │
//! ├───────────────────────────────────────────────────────────┤
//! │ status strip (28px)                                       │
//! └───────────────────────────────────────────────────────────┘
//! ```

pub mod app;
pub mod dialog;
pub mod groups;
pub mod motion;
pub mod server_panel;
pub mod sound_list;
pub mod theme;
pub mod top_bar;
pub mod types;
pub mod widgets;

pub use groups::{Group, GroupFilter, GroupId, Library, LibrarySound};

use crate::audio::AudioEngine;
use crate::gui::app::SoundSharedApp;
use crate::gui::types::{AppState, AudioCommand};
use crossbeam_channel::Receiver;
use eframe::{egui, App, CreationContext, Frame, NativeOptions};
use egui::{CentralPanel, Rounding, TopBottomPanel, ViewportBuilder};
use std::sync::{Arc, Mutex};
use theme::{Metrics, Palette};

/// Main GUI application struct — entry point expected by main.rs.
pub struct GuiApp {
    state: SoundSharedApp,
}

impl GuiApp {
    pub fn new(
        app_state: AppState,
        audio_engine: Arc<Mutex<Option<AudioEngine>>>,
        audio_cmd_rx: Receiver<AudioCommand>,
        cc: &CreationContext<'_>,
    ) -> Self {
        install_theme(&cc.egui_ctx);

        Self {
            state: SoundSharedApp::new(app_state, audio_engine, audio_cmd_rx, cc),
        }
    }

    pub fn native_options() -> NativeOptions {
        NativeOptions {
            // Sized so the group rail, the board and the network column all fit
            // without the operator having to stretch the window first.
            viewport: ViewportBuilder::default()
                .with_inner_size([1280.0, 800.0])
                .with_min_inner_size([960.0, 620.0])
                .with_title("SoundShared — Broadcast Console"),
            centered: true,
            ..Default::default()
        }
    }
}

/// Applies the Red Noir style to the context.
fn install_theme(ctx: &egui::Context) {
    let style = theme::create_theme();
    ctx.set_style(style.clone());
    ctx.all_styles_mut(|s| {
        s.visuals = style.visuals.clone();
        s.spacing = style.spacing.clone();
        s.text_styles = style.text_styles.clone();
        s.interaction = style.interaction.clone();
    });
}

impl App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.state.update(ctx);

        // Whole-window noir surface + faint scanline texture (dropped when the
        // operator asked for reduced motion).
        paint_backdrop(ctx, self.state.reduced_motion());

        TopBottomPanel::top("top_bar")
            .exact_height(Metrics::TOPBAR_H)
            .frame(egui::Frame::none().fill(Palette::APP))
            .show(ctx, |ui| {
                top_bar::top_bar(ui, &mut self.state);
            });

        TopBottomPanel::bottom("status_strip")
            .exact_height(Metrics::STATUS_H)
            .frame(egui::Frame::none().fill(Palette::APP))
            .show(ctx, |ui| {
                top_bar::status_strip(ui, &self.state);
            });

        CentralPanel::default()
            .frame(egui::Frame::none().fill(Palette::APP))
            .show(ctx, |ui| {
                // The shell sizes both columns with explicit rects; the panels
                // never negotiate the cursor themselves, so they always line up
                // to the pixel with the design: 24px outer padding, 24px gap,
                // 340px right column (all measured as painted border boxes).
                //
                // `console_panel` strokes the hairline *outside* its rect, so a
                // rect here is the FILL box: it is inset by one stroke from the
                // painted border box. Getting this wrong is what previously left
                // the right column 15px off the window edge while the left one
                // sat at 24px.
                const STROKE: f32 = 1.0;

                let content = ui.available_rect_before_wrap().shrink(Metrics::APP_PAD);
                let inner = egui::Rect::from_min_max(
                    egui::pos2(content.left() + STROKE, content.top() + STROKE),
                    egui::pos2(content.right() - STROKE, content.bottom() - STROKE),
                );

                let right_fill_w = Metrics::RIGHT_COL_W - 2.0 * STROKE;
                let right_rect = egui::Rect::from_min_max(
                    egui::pos2(inner.right() - right_fill_w, inner.top()),
                    inner.max,
                );
                let left_rect = egui::Rect::from_min_max(
                    inner.min,
                    egui::pos2(
                        right_rect.left() - Metrics::COL_GAP - 2.0 * STROKE,
                        inner.bottom(),
                    ),
                );

                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(left_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                    |ui| {
                        sound_list::sound_library_panel(ui, &mut self.state, left_rect.size());
                    },
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(right_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                    |ui| {
                        server_panel::server_panel(ui, &mut self.state, right_rect.size());
                    },
                );
            });

        // Modal dialogs (rename / move) painted above everything.
        dialog::show_dialogs(ctx, &mut self.state);

        if self.state.tooltip_requested() {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.state.persist_library();
        tracing::info!("SoundShared GUI shutting down");
    }
}

/// Paints the near-black backdrop and, unless motion is reduced, a very faint
/// red scanline texture.
///
/// The scanline is purely decorative (~2.5% opacity) and sits on the
/// background layer, so no widget is drawn on top of it. Reduced motion drops
/// the texture and keeps the flat surface.
fn paint_backdrop(ctx: &egui::Context, reduced_motion: bool) {
    let screen = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(screen, Rounding::ZERO, Palette::APP);

    if reduced_motion {
        return;
    }

    let color = theme::alpha(Palette::ROSE_600, 7);
    let mut y = screen.top();
    while y < screen.bottom() {
        painter.hline(screen.x_range(), y, egui::Stroke::new(1.0_f32, color));
        y += 3.0;
    }
}

/// Run the GUI application (used by main.rs).
pub fn run_gui(
    app_state: AppState,
    audio_engine: Arc<Mutex<Option<AudioEngine>>>,
    audio_cmd_rx: Receiver<AudioCommand>,
) -> Result<(), eframe::Error> {
    let options = GuiApp::native_options();
    eframe::run_native(
        "SoundShared",
        options,
        Box::new(move |cc| {
            Ok(Box::new(GuiApp::new(
                app_state,
                audio_engine,
                audio_cmd_rx,
                cc,
            )))
        }),
    )
}
