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

        // Whole-window noir surface + faint scanline texture.
        paint_backdrop(ctx);

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
                // The shell sizes both columns; the panels never negotiate the
                // cursor themselves, so they always line up to the pixel.
                let content = ui.available_rect_before_wrap().shrink(Metrics::APP_PAD);
                let height = content.height();
                let right_w = Metrics::RIGHT_COL_W;
                let left_w = (content.width() - right_w - Metrics::COL_GAP).max(320.0);

                // `scope_builder` keeps the inset rect, so the console sits on the
                // window padding exactly like the mockup instead of hugging the edges.
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(content)
                        .layout(egui::Layout::left_to_right(egui::Align::Min)),
                    |ui| {
                        sound_list::sound_library_panel(
                            ui,
                            &mut self.state,
                            egui::vec2(left_w, height),
                        );
                        ui.add_space(Metrics::COL_GAP);
                        server_panel::server_panel(
                            ui,
                            &mut self.state,
                            egui::vec2(right_w, height),
                        );
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

/// Paints the near-black backdrop and a very faint red scanline texture.
///
/// The scanline is purely decorative (~2.5% opacity) and sits on the
/// background layer, so no widget is drawn on top of it.
fn paint_backdrop(ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(screen, Rounding::ZERO, Palette::APP);

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
