use iced::widget::{button, container};
use iced::{Background, Border, Color, Font, Theme};
use std::sync::LazyLock;

pub const FONT_SIZE: f32 = 14.0;
pub const LINE_HEIGHT_PX: f32 = 20.0;

pub static EDITOR_FONT: LazyLock<Font> = LazyLock::new(|| {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    for &name in &["TX-02", "JetBrains Mono"] {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        if db.query(&query).is_some() {
            eprintln!("lst: using font '{name}'");
            return Font::with_name(name);
        }
    }

    eprintln!("lst: using system monospace font");
    Font::MONOSPACE
});

pub fn flat_btn(bg: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => Color {
                a: bg.a,
                r: (bg.r + 0.05).min(1.0),
                g: (bg.g + 0.05).min(1.0),
                b: (bg.b + 0.05).min(1.0),
            },
            _ => bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default().rounded(0),
            ..button::Style::default()
        }
    }
}

pub fn solid_bg(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(color)),
        ..container::Style::default()
    }
}
