use crate::gui::app::{SoundSharedApp, ServerStatus as GuiServerStatus};
use crate::gui::theme::*;
use egui::{Align, Color32, Context, Layout, RichText, Stroke, Ui, Vec2};

/// Renders the server control panel
pub fn server_panel(ui: &mut Ui, state: &mut SoundSharedApp) {
    card_panel(ui, |ui| {
        section_header(ui, "Network Server", None::<fn(&mut Ui)>);

        ui.add_space(SoundSharedSpacing::SM);

        let server_state = state.get_server_state();

        // Status row
        ui.horizontal(|ui| {
            ui.label(RichText::new("Status:").size(13.0).color(SoundSharedColors::TEXT_SECONDARY));
            ui.add_space(SoundSharedSpacing::SM);

            let (status_color, status_text): (Color32, &str) = match &server_state.status {
                GuiServerStatus::Stopped => (SoundSharedColors::STATUS_STOPPED, "Stopped"),
                GuiServerStatus::Starting => (SoundSharedColors::STATUS_STARTING, "Starting…"),
                GuiServerStatus::Running => (SoundSharedColors::STATUS_RUNNING, "Running"),
                GuiServerStatus::Error(msg) => (SoundSharedColors::STATUS_ERROR, Box::leak(format!("Error: {}", msg).into_boxed_str())),
            };

            status_indicator(ui, status_color, status_text);
        });

        ui.add_space(SoundSharedSpacing::MD);

        // Server info when running
        if matches!(server_state.status, GuiServerStatus::Running) {
            if let Some(url) = &server_state.bind_url {
                // URL row
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Listen URL:").size(12.0).color(SoundSharedColors::TEXT_SECONDARY));
                    ui.add_space(SoundSharedSpacing::SM);

                    let url_text = RichText::new(url)
                        .size(12.0)
                        .color(SoundSharedColors::ACCENT)
                        .monospace();

                    let response = ui.add(egui::Label::new(url_text).sense(egui::Sense::click()));
                    if response.clicked() {
                        ui.output_mut(|o| o.copied_text = url.clone());
                        state.set_copied_feedback(true);
                    }

                    ui.add_space(SoundSharedSpacing::SM);

                    if secondary_button(ui, "📋 Copy").clicked() {
                        ui.output_mut(|o| o.copied_text = url.clone());
                        state.set_copied_feedback(true);
                    }

                    if state.copied_feedback() {
                        ui.add_space(SoundSharedSpacing::SM);
                        ui.label(RichText::new("✓ Copied!").color(SoundSharedColors::SUCCESS).size(11.0));
                    }
                });

                ui.add_space(SoundSharedSpacing::SM);

                // Listener count
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Connected Listeners:").size(12.0).color(SoundSharedColors::TEXT_SECONDARY));
                    ui.add_space(SoundSharedSpacing::SM);

                    let count = server_state.listener_count;
                    let count_text = if count == 1 { "1 listener" } else { &format!("{} listeners", count) };

                    ui.label(
                        RichText::new(count_text)
                            .size(15.0)
                            .strong()
                            .color(if count > 0 { SoundSharedColors::SUCCESS } else { SoundSharedColors::TEXT_MUTED })
                    );

                    ui.add_space(SoundSharedSpacing::SM);

                    // Visual listener dots
                    if count > 0 {
                        ui.horizontal(|ui| {
                            for i in 0..count.min(5) {
                                let color = if i < count { SoundSharedColors::SUCCESS } else { SoundSharedColors::BORDER_PRIMARY };
                                let rect = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover()).0;
                                ui.painter().circle_filled(rect.center(), 4.0, color);
                            }
                            if count > 5 {
                                ui.label(RichText::new(format!("+{}", count - 5)).color(SoundSharedColors::TEXT_MUTED).size(10.0));
                            }
                        });
                    }
                });
            }

            ui.add_space(SoundSharedSpacing::LG);
            ui.separator();
            ui.add_space(SoundSharedSpacing::MD);

            // Stop server button
            ui.horizontal(|ui| {
                if danger_button(ui, "🛑 Stop Server").clicked() {
                    state.request_stop_server();
                }
            });
        } else {
            // Stopped / Error state — start button
            ui.horizontal(|ui| {
                match server_state.status {
                    GuiServerStatus::Stopped | GuiServerStatus::Error(_) => {
                        if primary_button(ui, "🚀 Start Server").clicked() {
                            state.request_start_server();
                        }

                        if matches!(server_state.status, GuiServerStatus::Error(_)) {
                            ui.add_space(SoundSharedSpacing::SM);
                            ui.label(RichText::new("Click to retry").color(SoundSharedColors::TEXT_MUTED).size(12.0));
                        }
                    }
                    GuiServerStatus::Starting => {
                        ui.add_enabled(false, egui::Button::new("⏳ Starting…"));
                        ui.add_space(SoundSharedSpacing::SM);
                        ui.spinner();
                    }
                    GuiServerStatus::Running => {
                        if danger_button(ui, "🛑 Stop Server").clicked() {
                            state.request_stop_server();
                        }
                    }
                }
            });

            ui.add_space(SoundSharedSpacing::LG);
            ui.separator();
            ui.add_space(SoundSharedSpacing::SM);

            // Configuration
            ui.label(RichText::new("Configuration").size(13.0).strong());
            ui.add_space(SoundSharedSpacing::SM);

            // Port
            ui.horizontal(|ui| {
                ui.label(RichText::new("Port:").size(12.0));
                ui.add_space(SoundSharedSpacing::SM);

                let mut port = state.get_server_port() as i32;
                if ui.add(
                    egui::DragValue::new(&mut port)
                        .range(1024..=65535)
                        .speed(1)
                ).changed() {
                    state.set_server_port(port as u16);
                }
            });

            ui.add_space(SoundSharedSpacing::XS);

            // Bind address
            ui.horizontal(|ui| {
                ui.label(RichText::new("Bind Address:").size(12.0));
                ui.add_space(SoundSharedSpacing::SM);

                let mut bind_addr = state.get_server_bind_addr();
                if ui.text_edit_singleline(&mut bind_addr).changed() {
                    state.set_server_bind_addr(bind_addr);
                }
            });
        }
    });
}

/// Compact server status for top bar
pub fn compact_server_status(ui: &mut Ui, state: &SoundSharedApp) {
    let server_state = state.get_server_state();

    let (color, text): (Color32, &str) = match &server_state.status {
        GuiServerStatus::Stopped => (SoundSharedColors::STATUS_STOPPED, "Server: Stopped"),
        GuiServerStatus::Starting => (SoundSharedColors::STATUS_STARTING, "Server: Starting…"),
        GuiServerStatus::Running => (SoundSharedColors::STATUS_RUNNING, "Server: Running"),
        GuiServerStatus::Error(_) => (SoundSharedColors::STATUS_ERROR, "Server: Error"),
    };

    ui.horizontal(|ui| {
        let rect = ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover()).0;
        ui.painter().circle_filled(rect.center(), 4.0, color);
        ui.add_space(SoundSharedSpacing::XS);
        ui.label(RichText::new(text).size(11.0).color(SoundSharedColors::TEXT_SECONDARY));
    });
}