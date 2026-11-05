//! `02 / NETWORK` — the broadcast server column.
//!
//! Fixed 340px column, mirroring the mockup:
//!
//! ```text
//! 02 / NETWORK
//! NETWORK SERVER
//! ● LIVE
//! BROADCASTING TO 3 LISTENERS
//! ─────────────────────────────
//! LISTEN URL   [ http://192.168.1.245:8080  ⧉ ]
//! LISTENERS    03  ▮▮▮▯▯▯▯▯   PEAK 3
//! ─────────────────────────────
//! [ ■ STOP SERVER ] [ ⧉ COPY LINK ]
//! ─────────────────────────────
//! CONFIGURATION  PORT 8080   BIND 0.0.0.0
//! WS / 48 kHz / STEREO
//! ```

use crate::gui::app::{ServerStatus as GuiServerStatus, SoundSharedApp};
use crate::gui::theme::{
    alpha, console_panel, hairline, micro_label, micro_label_accent, panel_title, telemetry,
    Metrics, Palette,
};
use crate::gui::widgets::{glyph, occupancy_cells, signal_dot, BtnKind, ConsoleButton};
use egui::{Align, Layout, RichText, Sense, Ui, Vec2};

/// Renders the network server panel at the given size.
pub fn server_panel(ui: &mut Ui, state: &mut SoundSharedApp, size: Vec2) {
    console_panel(ui, size, |ui| {
        let server = state.get_server_state();

        // ---- Header ----
        ui.label(micro_label_accent("02 / Network"));
        ui.add_space(2.0);
        ui.label(panel_title("Network Server"));
        ui.add_space(Metrics::LG);

        // ---- Big status ----
        let (status_color, headline, subline, pulse) = match &server.status {
            GuiServerStatus::Running => (
                Palette::ROSE_500,
                "Live".to_string(),
                format!(
                    "Broadcasting to {} listener{}",
                    server.listener_count,
                    if server.listener_count == 1 { "" } else { "s" }
                ),
                true,
            ),
            GuiServerStatus::Starting => (
                Palette::AMBER_400,
                "Starting".to_string(),
                "Binding the listen socket".to_string(),
                true,
            ),
            GuiServerStatus::Error(message) => (
                Palette::ROSE_400,
                "Error".to_string(),
                crate::gui::widgets::elide(message, 34),
                false,
            ),
            GuiServerStatus::Stopped => (
                Palette::ZINC_500,
                "Off Air".to_string(),
                "Server stopped".to_string(),
                false,
            ),
        };

        ui.horizontal(|ui| {
            signal_dot(ui, status_color, 8.0, pulse);
            ui.add_space(Metrics::SM);
            ui.label(
                RichText::new(headline.to_uppercase())
                    .font(crate::gui::widgets::glyph_font(24.0))
                    .color(status_color)
                    .strong(),
            );
        });
        ui.add_space(Metrics::XS);
        ui.label(micro_label(subline));

        ui.add_space(Metrics::LG);
        hairline(ui);
        ui.add_space(Metrics::LG);

        // ---- Listen URL ----
        ui.label(micro_label("Listen URL"));
        ui.add_space(Metrics::SM);

        let url = server.bind_url.clone().unwrap_or_else(|| "—".to_string());
        let running = matches!(server.status, GuiServerStatus::Running);

        ui.horizontal(|ui| {
            let well_w = (ui.available_width() - 34.0).max(60.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(well_w, 40.0), Sense::hover());
            let painter = ui.painter();
            painter.rect(
                rect,
                egui::Rounding::same(Metrics::R_SM),
                Palette::WELL,
                egui::Stroke::new(1.0_f32, Palette::BORDER),
            );
            let color = if running {
                Palette::ROSE_500
            } else {
                Palette::ZINC_600
            };
            let galley = painter.layout_no_wrap(
                crate::gui::widgets::elide(&url, 30),
                crate::gui::widgets::glyph_font(12.0),
                color,
            );
            painter.galley(
                egui::pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
                galley,
                color,
            );

            ui.add_space(Metrics::XS);
            let copy = ConsoleButton::ghost("")
                .glyph(glyph::COPY)
                .width(30.0)
                .height(40.0)
                .enabled(running)
                .tooltip("Copy listen URL")
                .id_salt("copy_url_icon")
                .show(ui);
            if copy.clicked() && running {
                ui.output_mut(|o| o.copied_text = url.clone());
                state.set_copied_feedback(true);
            }
        });

        if state.copied_feedback() {
            ui.add_space(Metrics::XS);
            ui.label(
                RichText::new("COPIED TO CLIPBOARD")
                    .font(crate::gui::widgets::glyph_font(9.0))
                    .color(Palette::ROSE_400)
                    .extra_letter_spacing(1.0),
            );
        }

        ui.add_space(Metrics::LG);

        // ---- Listener occupancy ----
        ui.label(micro_label("Listeners"));
        ui.add_space(Metrics::SM);
        ui.horizontal(|ui| {
            ui.label(telemetry(
                format!("{:02}", server.listener_count),
                30.0,
                Palette::WHITE,
            ));
            ui.add_space(Metrics::MD);
            occupancy_cells(ui, server.listener_count.min(8), 8, 8.0, 6.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(micro_label(format!("Peak {}", server.listener_count)));
            });
        });

        ui.add_space(Metrics::LG);
        hairline(ui);
        ui.add_space(Metrics::LG);

        // ---- Controls ----
        // Both buttons live on one line, so each needs an explicit width: with no
        // width a `ConsoleButton` claims the whole row and pushes its neighbour
        // outside the panel.
        let copy_w = 112.0;
        let primary_w = (ui.available_width() - copy_w - Metrics::SM).max(96.0);
        ui.horizontal(|ui| {
            match &server.status {
                GuiServerStatus::Stopped | GuiServerStatus::Error(_) => {
                    let label = if matches!(server.status, GuiServerStatus::Error(_)) {
                        "Retry Server"
                    } else {
                        "Start Server"
                    };
                    if ConsoleButton::primary(label)
                        .glyph(glyph::PLAY)
                        .width(primary_w)
                        .height(40.0)
                        .id_salt("server_start")
                        .show(ui)
                        .clicked()
                    {
                        state.request_start_server();
                    }
                }
                GuiServerStatus::Starting => {
                    let _ = ConsoleButton::primary("Starting")
                        .glyph(glyph::REFRESH)
                        .width(primary_w)
                        .height(40.0)
                        .enabled(false)
                        .id_salt("server_starting")
                        .show(ui);
                }
                GuiServerStatus::Running => {
                    if ConsoleButton::danger("Stop Server")
                        .glyph(glyph::STOP)
                        .width(primary_w)
                        .height(40.0)
                        .id_salt("server_stop")
                        .show(ui)
                        .clicked()
                    {
                        state.request_stop_server();
                    }
                }
            }

            ui.add_space(Metrics::SM);
            if ConsoleButton::ghost("Copy Link")
                .glyph(glyph::COPY)
                .width(copy_w)
                .height(40.0)
                .enabled(running)
                .id_salt("server_copy")
                .show(ui)
                .clicked()
                && running
            {
                ui.output_mut(|o| o.copied_text = url.clone());
                state.set_copied_feedback(true);
            }
        });

        ui.add_space(Metrics::LG);
        hairline(ui);
        ui.add_space(Metrics::LG);

        // ---- Configuration ----
        ui.label(micro_label("Configuration"));
        ui.add_space(Metrics::SM);

        let config_enabled = !matches!(
            server.status,
            GuiServerStatus::Running | GuiServerStatus::Starting
        );
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(micro_label("Port"));
                ui.add_space(2.0);
                config_field(
                    ui,
                    &state.get_server_port().to_string(),
                    config_enabled,
                    80.0,
                );
            });
            ui.add_space(Metrics::SM);
            ui.vertical(|ui| {
                ui.label(micro_label("Bind Address"));
                ui.add_space(2.0);
                config_field(ui, &state.get_server_bind_addr(), config_enabled, 120.0);
            });
        });

        if config_enabled {
            ui.add_space(Metrics::XS);
            ui.label(micro_label("Editable while stopped"));
        } else {
            ui.add_space(Metrics::XS);
            ui.label(micro_label("Stop the server to edit"));
        }

        ui.add_space(Metrics::LG);
        hairline(ui);
        ui.add_space(Metrics::MD);
        ui.label(micro_label("WS / 48 kHz / Stereo"));
    });
}

/// Read-only telemetry field. The values are edited in the settings dialog;
/// here they are shown as the console's inset wells.
fn config_field(ui: &mut Ui, value: &str, enabled: bool, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::Rounding::same(Metrics::R_SM),
        Palette::WELL,
        egui::Stroke::new(1.0_f32, Palette::BORDER),
    );
    let color = if enabled {
        Palette::WHITE
    } else {
        Palette::ZINC_600
    };
    let galley = painter.layout_no_wrap(
        value.to_string(),
        crate::gui::widgets::glyph_font(12.0),
        color,
    );
    painter.galley(
        egui::pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// One-line server status for the command bar or a compact layout.
pub fn compact_server_status(ui: &mut Ui, state: &SoundSharedApp) {
    let server = state.get_server_state();
    let (color, text) = match &server.status {
        GuiServerStatus::Stopped => (Palette::ZINC_500, "Server: Stopped"),
        GuiServerStatus::Starting => (Palette::AMBER_400, "Server: Starting"),
        GuiServerStatus::Running => (Palette::ROSE_500, "Server: Running"),
        GuiServerStatus::Error(_) => (Palette::ROSE_400, "Server: Error"),
    };

    ui.horizontal(|ui| {
        signal_dot(
            ui,
            color,
            8.0,
            matches!(server.status, GuiServerStatus::Running),
        );
        ui.add_space(Metrics::XS);
        ui.label(micro_label(text));
        let _ = BtnKind::Ghost;
        let _ = alpha;
    });
}
