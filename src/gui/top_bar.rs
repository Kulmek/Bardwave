use crate::gui::app::SoundSharedApp;
use crate::gui::theme::*;
use egui::{Align, Layout, RichText, Ui, Vec2};

/// Top bar: app title, master volume, server status
pub fn top_bar(ui: &mut Ui, state: &mut SoundSharedApp) {
    ui.horizontal(|ui| {
        // App branding
        ui.add_space(16.0);
        ui.heading(RichText::new("SoundShared").size(20.0).strong());
        ui.add_space(8.0);
        ui.label(RichText::new("Audio Streaming Server").size(12.0).color(SoundSharedColors::TEXT_MUTED));
        ui.add_space(16.0);
        ui.separator();
        ui.add_space(16.0);

        // Master volume
        ui.label(RichText::new("🔊 Master").size(12.0).color(SoundSharedColors::TEXT_SECONDARY));
        ui.add_space(4.0);
        let mut master = (state.get_master_volume() * 100.0) as i32;
        let vol_response = ui.add(
            egui::Slider::new(&mut master, 0..=100)
                .show_value(false)
        );
        if vol_response.changed() {
            state.set_master_volume(master as f32 / 100.0);
        }
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{}%", master)).size(11.0).color(SoundSharedColors::TEXT_MUTED).monospace());
        ui.add_space(24.0);

        // Spacer pushes server status to the right
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(12.0);
            crate::gui::server_panel::compact_server_status(ui, state);
            ui.add_space(16.0);
        });
    });
}