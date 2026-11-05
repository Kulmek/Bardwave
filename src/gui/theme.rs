use egui::{Color32, FontId, Rounding, Shadow, Stroke, Style, Visuals, WidgetText};

/// SoundShared color palette — Red Noir
pub struct SoundSharedColors;

impl SoundSharedColors {
    // Brand — deep crimson red
    pub const PRIMARY: Color32 = Color32::from_rgb(0xE1, 0x1D, 0x48);      // Red Noir red
    pub const PRIMARY_HOVER: Color32 = Color32::from_rgb(0x99, 0x1B, 0x1B);
    pub const PRIMARY_LIGHT: Color32 = Color32::from_rgb(0xF4, 0x3F, 0x5E);

    // Accent — crimson variant
    pub const ACCENT: Color32 = Color32::from_rgb(0xE1, 0x1D, 0x48);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x99, 0x1B, 0x1B);

    // Success / playing — red noir (consistent with theme)
    pub const SUCCESS: Color32 = Color32::from_rgb(0xE1, 0x1D, 0x48);
    pub const SUCCESS_HOVER: Color32 = Color32::from_rgb(0x99, 0x1B, 0x1B);

    // Danger / stop / error — darker crimson
    pub const DANGER: Color32 = Color32::from_rgb(0x99, 0x1B, 0x1B);
    pub const DANGER_HOVER: Color32 = Color32::from_rgb(0x7F, 0x1D, 0x1D);

    // Warning — amber
    pub const WARNING: Color32 = Color32::from_rgb(0xFF, 0xB8, 0x00);

    // Backgrounds — deep noir black
    pub const BG_PRIMARY: Color32 = Color32::from_rgb(0x0A, 0x0A, 0x0A);   // Deep noir black
    pub const BG_SECONDARY: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);  // Panel bg
    pub const BG_TERTIARY: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);   // Card bg
    pub const BG_HOVER: Color32 = Color32::from_rgb(0x24, 0x24, 0x24);      // Hover state
    pub const BG_INPUT: Color32 = Color32::from_rgb(0x15, 0x15, 0x15);      // Input bg

    // Borders — subtle noir with crimson focus
    pub const BORDER_PRIMARY: Color32 = Color32::from_rgb(0x27, 0x27, 0x2A);
    pub const BORDER_FOCUS: Color32 = Color32::from_rgb(0xE1, 0x1D, 0x48);

    // Text — off-white hierarchy
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xF5, 0xF5, 0xF7);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xA1, 0xA1, 0xAA);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x71, 0x71, 0x7A);
    pub const TEXT_ON_PRIMARY: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);

    // Status — crimson + noir
    pub const STATUS_STOPPED: Color32 = Color32::from_rgb(0x71, 0x71, 0x7A);
    pub const STATUS_STARTING: Color32 = Color32::from_rgb(0xFF, 0xB8, 0x00);
    pub const STATUS_RUNNING: Color32 = Color32::from_rgb(0xE1, 0x1D, 0x48);
    pub const STATUS_ERROR: Color32 = Color32::from_rgb(0x99, 0x1B, 0x1B);
}

/// Layout spacing
pub struct SoundSharedSpacing;

impl SoundSharedSpacing {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 16.0;
    pub const LG: f32 = 24.0;
    pub const XL: f32 = 32.0;
    pub const PANEL_PADDING: f32 = 20.0;
    pub const ITEM_GAP: f32 = 10.0;
    pub const SECTION_GAP: f32 = 24.0;
}

/// Corner radius
pub struct SoundSharedRadius;

impl SoundSharedRadius {
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 10.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Create the SoundShared Red Noir theme
pub fn create_theme() -> Style {
    let mut style = Style::default();
    let visuals = &mut style.visuals;

    // Window & panel fills — noir black
    visuals.window_fill = SoundSharedColors::BG_PRIMARY;
    visuals.panel_fill = SoundSharedColors::BG_SECONDARY;
    visuals.faint_bg_color = SoundSharedColors::BG_TERTIARY;
    visuals.extreme_bg_color = SoundSharedColors::BG_PRIMARY;

    // Widget states
    visuals.widgets.noninteractive.bg_fill = SoundSharedColors::BG_TERTIARY;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SoundSharedColors::BORDER_PRIMARY);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, SoundSharedColors::TEXT_PRIMARY);
    visuals.widgets.noninteractive.rounding = Rounding::same(SoundSharedRadius::MD);

    visuals.widgets.inactive.bg_fill = SoundSharedColors::BG_TERTIARY;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, SoundSharedColors::BORDER_PRIMARY);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, SoundSharedColors::TEXT_PRIMARY);
    visuals.widgets.inactive.rounding = Rounding::same(SoundSharedRadius::MD);

    visuals.widgets.hovered.bg_fill = SoundSharedColors::BG_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, SoundSharedColors::BORDER_FOCUS);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, SoundSharedColors::TEXT_PRIMARY);
    visuals.widgets.hovered.rounding = Rounding::same(SoundSharedRadius::MD);

    visuals.widgets.active.bg_fill = SoundSharedColors::PRIMARY;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, SoundSharedColors::PRIMARY);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, SoundSharedColors::TEXT_ON_PRIMARY);
    visuals.widgets.active.rounding = Rounding::same(SoundSharedRadius::MD);

    visuals.widgets.open.bg_fill = SoundSharedColors::BG_TERTIARY;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, SoundSharedColors::BORDER_PRIMARY);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, SoundSharedColors::TEXT_PRIMARY);
    visuals.widgets.open.rounding = Rounding::same(SoundSharedRadius::MD);

    // Selection — crimson
    visuals.selection.bg_fill = SoundSharedColors::PRIMARY.gamma_multiply(0.2);
    visuals.selection.stroke = Stroke::new(1.0, SoundSharedColors::PRIMARY);

    // Hyperlink — crimson
    visuals.hyperlink_color = SoundSharedColors::ACCENT;

    // Text
    visuals.override_text_color = Some(SoundSharedColors::TEXT_PRIMARY);

    // Window
    visuals.window_rounding = Rounding::same(SoundSharedRadius::LG);
    visuals.window_stroke = Stroke::new(1.0, SoundSharedColors::BORDER_PRIMARY);
    visuals.window_shadow = Shadow {
        offset: egui::vec2(0.0, 8.0),
        blur: 24.0,
        spread: 0.0,
        color: Color32::from_black_alpha(140),
    };

    visuals.popup_shadow = Shadow {
        offset: egui::vec2(0.0, 4.0),
        blur: 12.0,
        spread: 0.0,
        color: Color32::from_black_alpha(120),
    };

    visuals.resize_corner_size = 12.0;

    // Text styles
    style.text_styles = [
        (egui::TextStyle::Heading, FontId::new(28.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Body, FontId::new(15.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Monospace, FontId::new(13.0, egui::FontFamily::Monospace)),
        (egui::TextStyle::Button, FontId::new(14.0, egui::FontFamily::Proportional)),
        (egui::TextStyle::Small, FontId::new(12.0, egui::FontFamily::Proportional)),
    ].into();

    // Spacing
    style.spacing.item_spacing = egui::vec2(SoundSharedSpacing::SM, SoundSharedSpacing::SM);
    style.spacing.window_margin = egui::Margin::same(SoundSharedSpacing::PANEL_PADDING);
    style.spacing.button_padding = egui::vec2(SoundSharedSpacing::MD, SoundSharedSpacing::SM);
    style.spacing.slider_width = 140.0;
    style.spacing.text_edit_width = 220.0;
    style.spacing.icon_width = 20.0;
    style.spacing.icon_width_inner = 16.0;

    style.interaction.selectable_labels = false;
    style.interaction.tooltip_delay = 0.3;
    style.animation_time = 0.15;

    style
}

/// Primary action button (brand crimson)
pub fn primary_button(ui: &mut egui::Ui, text: impl Into<WidgetText>) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .fill(SoundSharedColors::PRIMARY)
            .stroke(Stroke::NONE)
            .rounding(SoundSharedRadius::MD)
            .min_size(egui::vec2(110.0, 36.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Success / play button (crimson)
pub fn success_button(ui: &mut egui::Ui, text: impl Into<WidgetText>) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .fill(SoundSharedColors::SUCCESS)
            .stroke(Stroke::NONE)
            .rounding(SoundSharedRadius::MD)
            .min_size(egui::vec2(90.0, 32.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Danger / stop button (dark crimson)
pub fn danger_button(ui: &mut egui::Ui, text: impl Into<WidgetText>) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .fill(SoundSharedColors::DANGER)
            .stroke(Stroke::NONE)
            .rounding(SoundSharedRadius::MD)
            .min_size(egui::vec2(90.0, 32.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Secondary / ghost button
pub fn secondary_button(ui: &mut egui::Ui, text: impl Into<WidgetText>) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .fill(SoundSharedColors::BG_TERTIARY)
            .stroke(Stroke::new(1.0, SoundSharedColors::BORDER_PRIMARY))
            .rounding(SoundSharedRadius::MD)
            .min_size(egui::vec2(90.0, 32.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Status indicator dot with label
pub fn status_indicator(ui: &mut egui::Ui, color: Color32, label: impl Into<WidgetText>) {
    ui.horizontal(|ui| {
        let rect = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover()).0;
        ui.painter().circle_filled(rect.center(), 5.0, color);
        ui.add_space(SoundSharedSpacing::XS);
        ui.label(label);
    });
}

/// Modern card panel (noir)
pub fn card_panel(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(SoundSharedColors::BG_TERTIARY)
        .stroke(Stroke::new(1.0, SoundSharedColors::BORDER_PRIMARY))
        .rounding(SoundSharedRadius::LG)
        .inner_margin(SoundSharedSpacing::PANEL_PADDING)
        .show(ui, add_contents);
}

/// Section header with optional right-aligned action
pub fn section_header(ui: &mut egui::Ui, title: &str, action: Option<impl FnOnce(&mut egui::Ui)>) {
    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(title).size(16.0).strong());
        if let Some(action_fn) = action {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), action_fn);
        }
    });
    ui.add_space(SoundSharedSpacing::XS);
    ui.separator();
    ui.add_space(SoundSharedSpacing::SM);
}
