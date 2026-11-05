use crate::gui::app::{SoundSharedApp, SoundEntry};
use crate::gui::theme::*;
use egui::{Align, Layout, RichText, Sense, Stroke, Ui};

/// Renders the sound library panel
pub fn sound_list_panel(ui: &mut Ui, state: &mut SoundSharedApp) {
    card_panel(ui, |ui| {
        // Header with Add Sound button
        ui.horizontal(|ui| {
            ui.heading("Sound Library");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary_button(ui, "➕ Add Sound").clicked() {
                    state.open_file_dialog();
                }
            });
        });

        ui.add_space(SoundSharedSpacing::SM);
        ui.separator();
        ui.add_space(SoundSharedSpacing::SM);

        let sounds = state.get_sounds();

        if sounds.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(SoundSharedSpacing::LG);
                ui.label(RichText::new("No sounds loaded yet").color(SoundSharedColors::TEXT_MUTED));
                ui.add_space(SoundSharedSpacing::XS);
                ui.label(RichText::new("Click \"Add Sound\" to load audio files").size(12.0).color(SoundSharedColors::TEXT_MUTED));
                ui.add_space(SoundSharedSpacing::LG);
            });
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([true, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = SoundSharedSpacing::XS;

                    for sound in sounds.iter() {
                        sound_list_item(ui, state, sound);
                    }
                });
        }

        ui.add_space(SoundSharedSpacing::MD);

        // Master volume slider section
        ui.separator();
        ui.add_space(SoundSharedSpacing::SM);

        ui.horizontal(|ui| {
            ui.label(RichText::new("🔊 Master Volume").size(13.0).strong().color(SoundSharedColors::TEXT_PRIMARY));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let master_vol = state.get_master_volume();
                let mut vol_percent = (master_vol * 100.0) as i32;
                let response = ui.add(
                    egui::Slider::new(&mut vol_percent, 0..=100)
                        .text("%")
                        .show_value(true)
                );
                if response.changed() {
                    state.set_master_volume(vol_percent as f32 / 100.0);
                }
            });
        });
    });
}

/// Renders a single sound entry in the list
fn sound_list_item(ui: &mut Ui, state: &mut SoundSharedApp, sound: &SoundEntry) {
    let is_playing = sound.playing;
    let sound_id = sound.id;
    let name = sound.name.clone();

    egui::Frame::none()
        .fill(if is_playing {
            SoundSharedColors::PRIMARY.gamma_multiply(0.12)
        } else {
            SoundSharedColors::BG_TERTIARY
        })
        .stroke(Stroke::new(
            1.0,
            if is_playing {
                SoundSharedColors::PRIMARY
            } else {
                SoundSharedColors::BORDER_PRIMARY
            }
        ))
        .rounding(SoundSharedRadius::MD)
        .inner_margin(SoundSharedSpacing::MD)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Play / Stop button
                let btn = if is_playing {
                    danger_button(ui, "⏹ Stop")
                } else {
                    success_button(ui, "▶ Play")
                };

                if btn.clicked() {
                    if is_playing {
                        state.request_stop_sound(sound_id);
                    } else {
                        state.request_play_sound(sound_id);
                    }
                }

                ui.add_space(SoundSharedSpacing::SM);

                // Sound name (truncated with hover tooltip)
                let name_display = if name.len() > 42 {
                    format!("{}…", &name[..39])
                } else {
                    name.clone()
                };

                let name_color = if is_playing {
                    SoundSharedColors::PRIMARY_LIGHT
                } else {
                    SoundSharedColors::TEXT_PRIMARY
                };

                ui.add_sized([220.0, 24.0], egui::Label::new(
                    RichText::new(name_display).color(name_color).size(13.0)
                ).sense(Sense::hover()))
                .on_hover_text(&name);

                ui.add_space(SoundSharedSpacing::SM);

                // Volume slider (compact)
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Remove button
                    if secondary_button(ui, "🗑 Remove").clicked() {
                        state.request_remove_sound(sound_id);
                    }

                    ui.add_space(SoundSharedSpacing::SM);

                    // Volume slider
                    let mut vol_percent = (sound.volume * 100.0) as i32;
                    let vol_response = ui.add(
                        egui::Slider::new(&mut vol_percent, 0..=100)
                            .show_value(false)
                    );
                    if vol_response.changed() {
                        state.request_set_volume(sound_id, vol_percent as f32 / 100.0);
                    }

                    ui.add_space(SoundSharedSpacing::SM);
                    ui.label(RichText::new(format!("{}%", vol_percent)).size(11.0).color(SoundSharedColors::TEXT_MUTED).monospace());
                });
            });
        });
}

/// Compact sound list for smaller spaces
pub fn compact_sound_list(ui: &mut Ui, state: &mut SoundSharedApp) {
    let sounds = state.get_sounds();

    for sound in sounds.iter() {
        ui.horizontal(|ui| {
            let is_playing = sound.playing;
            let sound_id = sound.id;

            let btn = if is_playing {
                danger_button(ui, "⏹")
            } else {
                success_button(ui, "▶")
            };

            if btn.clicked() {
                if is_playing {
                    state.request_stop_sound(sound_id);
                } else {
                    state.request_play_sound(sound_id);
                }
            }

            let name_display = if sound.name.len() > 28 {
                format!("{}…", &sound.name[..25])
            } else {
                sound.name.clone()
            };

            let label_color = if is_playing { SoundSharedColors::PRIMARY } else { SoundSharedColors::TEXT_PRIMARY };
            ui.label(RichText::new(name_display).color(label_color).size(13.0));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("✕").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    state.request_remove_sound(sound_id);
                }

                let mut vol = (sound.volume * 100.0) as i32;
                if ui.add(egui::Slider::new(&mut vol, 0..=100).show_value(false)).changed() {
                    state.request_set_volume(sound_id, vol as f32 / 100.0);
                }
            });
        });
    }
}