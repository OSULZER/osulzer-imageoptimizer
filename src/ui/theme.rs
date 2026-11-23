//! Erscheinungsbild: Farben, Abstände, Schriftgrößen und die Hauptschaltfläche.

use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, Stroke, TextStyle};

/// Hausfarbe Lila-Magenta.
pub const ACCENT: Color32 = Color32::from_rgb(0xe8, 0x3e, 0x91);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xf0, 0x5c, 0xa4);
pub const ACCENT_PRESSED: Color32 = Color32::from_rgb(0xcc, 0x2f, 0x7c);
/// Hinterlegung der ausgewählten Listenzeile.
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x3d, 0x1f, 0x33);
pub const GOOD: Color32 = Color32::from_rgb(0x5f, 0xd3, 0x8d);

pub const PANEL: Color32 = Color32::from_rgb(0x17, 0x17, 0x1b);
pub const SURFACE: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x25);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x24, 0x24, 0x2b);
const CONTROL: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x32);
const CONTROL_HOVER: Color32 = Color32::from_rgb(0x33, 0x33, 0x3d);
const CONTROL_ACTIVE: Color32 = Color32::from_rgb(0x3b, 0x3b, 0x46);
const BORDER: Color32 = Color32::from_rgb(0x38, 0x38, 0x42);
pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xec, 0xf1);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0x9a, 0x9a, 0xa6);

const RADIUS: u8 = 7;

pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.global_style_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(12.0)),
            (TextStyle::Body, FontId::proportional(14.5)),
            (TextStyle::Button, FontId::proportional(14.5)),
            (TextStyle::Heading, FontId::proportional(19.0)),
            (TextStyle::Monospace, FontId::monospace(13.5)),
        ]
        .into();

        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(10.0, 8.0);
        s.button_padding = egui::vec2(14.0, 7.0);
        s.interact_size = egui::vec2(40.0, 32.0);
        s.menu_margin = Margin::same(8);
        s.window_margin = Margin::same(18);
        s.combo_width = 150.0;
        s.slider_width = 150.0;
        s.icon_width = 18.0;
        s.icon_width_inner = 10.0;
        s.icon_spacing = 8.0;

        // Texte sind keine Bedienelemente: sonst schlucken sie Klicks auf die Listenzeilen.
        style.interaction.selectable_labels = false;

        let v = &mut style.visuals;
        v.panel_fill = PANEL;
        v.window_fill = SURFACE;
        v.faint_bg_color = SURFACE;
        v.extreme_bg_color = Color32::from_rgb(0x10, 0x10, 0x13);
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.hyperlink_color = ACCENT;
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.5, Color32::WHITE);
        v.override_text_color = None;

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = SURFACE;
        w.noninteractive.weak_bg_fill = SURFACE;
        w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x2a, 0x2a, 0x31));
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = CornerRadius::same(RADIUS);

        for (state, fill, stroke) in [
            (&mut w.inactive, CONTROL, BORDER),
            (&mut w.hovered, CONTROL_HOVER, ACCENT_PRESSED),
            (&mut w.active, CONTROL_ACTIVE, ACCENT),
            (&mut w.open, CONTROL_HOVER, ACCENT),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0, stroke);
            state.fg_stroke = Stroke::new(1.5, TEXT);
            state.corner_radius = CornerRadius::same(RADIUS);
            state.expansion = 0.0;
        }
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    });
}

/// Rahmen für Dialoge: wie ein Aufklappmenü, aber mit großzügigem Innenabstand.
pub fn modal_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::popup(&ctx.global_style()).inner_margin(Margin::symmetric(28, 24))
}

/// Die Hauptschaltfläche in der Hausfarbe, gleich groß wie eine normale Schaltfläche mit diesem Text.
pub fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE);
    let padding = ui.spacing().button_padding;
    let size = egui::vec2(
        galley.size().x + 2.0 * padding.x,
        (galley.size().y + 2.0 * padding.y).max(ui.spacing().interact_size.y),
    );
    let (rect, response) = ui.allocate_exact_size(size, if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let fill = match (enabled, response.is_pointer_button_down_on(), response.hovered()) {
        (false, _, _) => ACCENT.gamma_multiply(0.35),
        (true, true, _) => ACCENT_PRESSED,
        (true, false, true) => ACCENT_HOVER,
        (true, false, false) => ACCENT,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(RADIUS), fill);
    let color = if enabled { Color32::WHITE } else { Color32::from_white_alpha(140) };
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, font, color);
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}
