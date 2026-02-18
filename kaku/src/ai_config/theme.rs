use ratatui::style::Color;
use std::sync::LazyLock;

pub(super) struct Theme {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub error: Color,
    pub text: Color,
    pub muted: Color,
    pub bg: Color,
    pub panel: Color,
}

pub(super) fn parse_hex(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    Color::Rgb(r, g, b)
}

pub(super) static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let json: serde_json::Value =
        serde_json::from_str(super::OPENCODE_THEME_JSON).unwrap_or_default();
    let defs = &json["defs"];
    let hex =
        |key: &str, fallback: &str| -> Color { parse_hex(defs[key].as_str().unwrap_or(fallback)) };
    Theme {
        primary: hex("primary", "#a277ff"),
        secondary: hex("secondary", "#61ffca"),
        accent: hex("accent", "#ffca85"),
        error: hex("error", "#ff6767"),
        text: hex("text", "#edecee"),
        muted: hex("muted", "#6b6b6b"),
        bg: hex("bg", "#15141b"),
        panel: hex("element", "#1f1d28"),
    }
});

#[allow(non_snake_case)]
pub(super) fn PURPLE() -> Color {
    THEME.primary
}
#[allow(non_snake_case)]
pub(super) fn GREEN() -> Color {
    THEME.secondary
}
#[allow(non_snake_case)]
pub(super) fn YELLOW() -> Color {
    THEME.accent
}
#[allow(non_snake_case)]
pub(super) fn RED() -> Color {
    THEME.error
}
#[allow(non_snake_case)]
pub(super) fn TEXT() -> Color {
    THEME.text
}
#[allow(non_snake_case)]
pub(super) fn MUTED() -> Color {
    THEME.muted
}
#[allow(non_snake_case)]
pub(super) fn BG() -> Color {
    THEME.bg
}
#[allow(non_snake_case)]
pub(super) fn PANEL() -> Color {
    THEME.panel
}
