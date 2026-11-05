pub mod app;
pub mod server_panel;
pub mod sound_list;
pub mod theme;
pub mod top_bar;
pub mod types;

use crate::audio::AudioEngine;
use crate::gui::app::SoundSharedApp;
use crate::gui::types::{AppState, AudioCommand};
use crate::gui::top_bar::top_bar;
use crossbeam_channel::Receiver;
use eframe::{egui, Frame, NativeOptions, App, CreationContext};
use egui::{CentralPanel, TopBottomPanel, ViewportBuilder};
use std::sync::{Arc, Mutex};

/// Main GUI application struct - entry point expected by main.rs
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
        cc.egui_ctx.set_style(crate::gui::theme::create_theme());

        Self {
            state: SoundSharedApp::new(app_state, audio_engine, audio_cmd_rx, cc),
        }
    }

    pub fn native_options() -> NativeOptions {
        NativeOptions {
            viewport: ViewportBuilder::default()
                .with_inner_size([1100.0, 750.0])
                .with_min_inner_size([900.0, 600.0])
                .with_title("SoundShared"),
            centered: true,
            ..Default::default()
        }
    }
}

impl App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.state.update(ctx);

        // Top bar — app title + global controls + server status
        TopBottomPanel::top("top_bar")
            .exact_height(64.0)
            .show(ctx, |ui| {
                top_bar(ui, &mut self.state);
            });

        // Central area — sound library (left) + server controls (right)
        CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_size_before_wrap();

            ui.horizontal(|ui| {
                // Left: Sound library — flexible width, takes available space
                ui.allocate_ui_with_layout(
                    egui::vec2(available.x * 0.62, available.y),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        crate::gui::sound_list::sound_list_panel(ui, &mut self.state);
                    },
                );

                ui.add_space(12.0);

                // Right: Server controls — fixed width, always visible on the right edge
                ui.allocate_ui_with_layout(
                    egui::vec2(340.0, available.y),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        crate::gui::server_panel::server_panel(ui, &mut self.state);
                    },
                );
            });
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        tracing::info!("SoundShared GUI shutting down");
    }
}

/// Run the GUI application (used by main.rs)
pub fn run_gui(
    app_state: AppState,
    audio_engine: Arc<Mutex<Option<AudioEngine>>>,
    audio_cmd_rx: Receiver<AudioCommand>,
) -> Result<(), eframe::Error> {
    let options = GuiApp::native_options();
    eframe::run_native(
        "SoundShared",
        options,
        Box::new(move |cc| Ok(Box::new(GuiApp::new(app_state, audio_engine, audio_cmd_rx, cc)))),
    )
}