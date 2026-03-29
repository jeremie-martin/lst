use iced::widget::{button, container};
use iced::{Background, Border, Color, Font, Theme};
use std::sync::LazyLock;

pub const FONT_SIZE: f32 = 14.0;
pub const LINE_HEIGHT_PX: f32 = 20.0;

pub struct EditorFont {
    pub font: Font,
    /// Advance width of a single monospace character at FONT_SIZE, in pixels.
    pub char_width: f32,
}

pub static EDITOR_FONT: LazyLock<EditorFont> = LazyLock::new(|| {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    for &name in &["TX-02", "JetBrains Mono"] {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        if let Some(id) = db.query(&query) {
            let char_width = measure_char_width(&db, id);
            eprintln!("lst: using font '{name}' (char_width={char_width:.2}px)");
            return EditorFont {
                font: Font::with_name(name),
                char_width,
            };
        }
    }

    eprintln!("lst: using system monospace font");
    EditorFont {
        font: Font::MONOSPACE,
        char_width: FONT_SIZE * 0.6,
    }
});

fn measure_char_width(db: &fontdb::Database, id: fontdb::ID) -> f32 {
    db.with_face_data(id, |data, index| {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let glyph_id = face.glyph_index('M')?;
        let advance = face.glyph_hor_advance(glyph_id)?;
        let upm = face.units_per_em();
        Some(FONT_SIZE * advance as f32 / upm as f32)
    })
    .flatten()
    .unwrap_or(FONT_SIZE * 0.6)
}

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
