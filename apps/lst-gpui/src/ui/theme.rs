use gpui::{font, px, Font, FontFallbacks, Pixels};

pub mod palette {
    pub const CHROME: u32 = 0x181818;
    pub const PANEL: u32 = 0x252526;
    pub const EDITOR: u32 = 0x1F1F1F;
    pub const CONTROL: u32 = 0x313131;
    pub const CONTROL_HOVER: u32 = 0x3C3C3C;
    pub const BORDER: u32 = 0x3C3C3C;
    pub const TEXT: u32 = 0xCCCCCC;
    pub const TEXT_SUBTLE: u32 = 0xA6A6A6;
    pub const TEXT_MUTED: u32 = 0x808080;
    pub const ACCENT_BLUE: u32 = 0x0078D4;
    pub const ERROR_RED: u32 = 0xF14C4C;
    pub const SELECTION_BLUE: u32 = 0x264F78;
    pub const SEARCH_MATCH: u32 = 0x3A3D41;
    pub const SEARCH_ACTIVE_MATCH: u32 = 0x6B4F1D;
    pub const CURRENT_LINE: u32 = 0x2A2D2E;
    pub const GUTTER: u32 = 0x181818;
    pub const SCROLLBAR_THUMB: u32 = 0x5A5A5A;
    pub const SCROLLBAR_THUMB_ACTIVE: u32 = 0x808080;

    pub const SYNTAX_BLUE: u32 = 0x569CD6;
    pub const SYNTAX_GREEN: u32 = 0x6A9955;
    pub const SYNTAX_ORANGE: u32 = 0xCE9178;
    pub const SYNTAX_YELLOW: u32 = 0xDCDCAA;
    pub const SYNTAX_GOLD: u32 = 0xD7BA7D;
    pub const SYNTAX_TEAL: u32 = 0x4EC9B0;
    pub const SYNTAX_LIGHT_BLUE: u32 = 0x9CDCFE;
    pub const SYNTAX_PURPLE: u32 = 0xC586C0;
    pub const SYNTAX_NUMBER: u32 = 0xB5CEA8;
}

pub mod role {
    use super::palette;

    pub const APP_BG: u32 = palette::CHROME;
    pub const PANEL_BG: u32 = palette::PANEL;
    pub const EDITOR_BG: u32 = palette::EDITOR;
    pub const CONTROL_BG: u32 = palette::CONTROL;
    pub const CONTROL_BG_HOVER: u32 = palette::CONTROL_HOVER;
    pub const BORDER: u32 = palette::BORDER;
    pub const TEXT: u32 = palette::TEXT;
    pub const TEXT_SUBTLE: u32 = palette::TEXT_SUBTLE;
    pub const TEXT_MUTED: u32 = palette::TEXT_MUTED;
    pub const ACCENT: u32 = palette::ACCENT_BLUE;
    pub const ERROR_TEXT: u32 = palette::ERROR_RED;
    pub const SELECTION_BG: u32 = palette::SELECTION_BLUE;
    pub const SEARCH_MATCH_BG: u32 = palette::SEARCH_MATCH;
    pub const SEARCH_ACTIVE_MATCH_BG: u32 = palette::SEARCH_ACTIVE_MATCH;
    pub const CARET: u32 = palette::TEXT;
    pub const CURRENT_LINE_BG: u32 = palette::CURRENT_LINE;
    pub const GUTTER_BG: u32 = palette::GUTTER;
    pub const SCROLLBAR_THUMB: u32 = palette::SCROLLBAR_THUMB;
    pub const SCROLLBAR_THUMB_ACTIVE: u32 = palette::SCROLLBAR_THUMB_ACTIVE;
}

pub mod syntax {
    use super::{palette, role};

    pub const COMMENT: u32 = palette::SYNTAX_GREEN;
    pub const STRING: u32 = palette::SYNTAX_ORANGE;
    pub const CONSTANT: u32 = palette::SYNTAX_NUMBER;
    pub const FUNCTION: u32 = palette::SYNTAX_YELLOW;
    pub const KEYWORD: u32 = palette::SYNTAX_BLUE;
    pub const OPERATOR: u32 = role::TEXT;
    pub const TYPE: u32 = palette::SYNTAX_TEAL;
    pub const TAG: u32 = palette::SYNTAX_BLUE;
    pub const TITLE: u32 = palette::SYNTAX_BLUE;
    pub const STRONG: u32 = palette::SYNTAX_BLUE;
    pub const EMPHASIS: u32 = palette::SYNTAX_PURPLE;
    pub const LITERAL: u32 = palette::SYNTAX_ORANGE;
    pub const REFERENCE: u32 = palette::SYNTAX_LIGHT_BLUE;
    pub const PROPERTY: u32 = palette::SYNTAX_LIGHT_BLUE;
    pub const ESCAPE: u32 = palette::SYNTAX_GOLD;
    pub const PUNCTUATION: u32 = role::TEXT_MUTED;
    pub const LABEL: u32 = palette::SYNTAX_LIGHT_BLUE;
}

pub mod typography {
    use super::{font, Font, FontFallbacks};
    use std::sync::OnceLock;

    pub const PRIMARY_FONT_FAMILY: &str = "TX-02";

    pub fn primary_font() -> Font {
        static FONT: OnceLock<Font> = OnceLock::new();

        FONT.get_or_init(|| {
            let mut font = font(PRIMARY_FONT_FAMILY);
            font.fallbacks = Some(FontFallbacks::from_fonts(vec![
                "JetBrains Mono".to_string(),
                ".ZedMono".to_string(),
                "Lilex".to_string(),
                "IBM Plex Mono".to_string(),
            ]));
            font
        })
        .clone()
    }
}

pub mod metrics {
    use super::{px, Pixels};

    pub const BASE_REM_SIZE: f32 = 16.0;
    pub const MIN_ZOOM_LEVEL: i32 = -4;
    pub const MAX_ZOOM_LEVEL: i32 = 8;
    pub const ZOOM_STEP: f32 = 1.1;

    pub const WINDOW_WIDTH: f32 = 1360.0;
    pub const WINDOW_HEIGHT: f32 = 860.0;
    pub const SHELL_GAP: f32 = 8.0;
    pub const SHELL_EDGE_PAD: f32 = SHELL_GAP;
    pub const STATUS_HEIGHT_PAD: f32 = 10.0;

    pub const TAB_HEIGHT: f32 = 30.0;
    pub const TAB_MIN_WIDTH: f32 = 128.0;
    pub const TAB_MAX_WIDTH: f32 = 220.0;
    pub const TAB_HORIZONTAL_PAD: f32 = 10.0;
    pub const TAB_SLOT_WIDTH: f32 = 18.0;
    pub const TAB_TEXT_SIZE: f32 = 12.0;
    pub const TAB_TEXT_LINE_HEIGHT: f32 = 16.0;
    pub const ICON_BUTTON_SIZE: f32 = 16.0;

    pub const INPUT_HEIGHT: f32 = 30.0;
    pub const INPUT_HORIZONTAL_PAD: f32 = 12.0;
    pub const INPUT_TEXT_SIZE: f32 = 12.0;
    pub const INPUT_TEXT_LINE_HEIGHT: f32 = 18.0;

    pub const ROW_HEIGHT: f32 = 22.0;
    pub const GUTTER_WIDTH: f32 = 58.0;
    pub const CODE_FONT_SIZE: f32 = 13.0;
    pub const CURSOR_WIDTH: f32 = 2.0;
    pub const VIEWPORT_OVERSCAN_LINES: usize = 6;
    pub const EDITOR_LEFT_PAD: f32 = 18.0;
    pub const GUTTER_LEFT_PAD: f32 = 12.0;
    pub const WRAP_CHAR_WIDTH_FALLBACK: f32 = 7.8;
    pub const SCROLLBAR_TRACK_WIDTH: f32 = 10.0;
    pub const SCROLLBAR_THUMB_WIDTH: f32 = 4.0;
    pub const SCROLLBAR_EDGE_PAD: f32 = 3.0;
    pub const SCROLLBAR_MIN_THUMB_HEIGHT: f32 = 24.0;

    pub fn zoom_scale(level: i32) -> f32 {
        ZOOM_STEP.powi(level.clamp(MIN_ZOOM_LEVEL, MAX_ZOOM_LEVEL))
    }

    pub fn px_for_scale(value: f32, scale: f32) -> Pixels {
        px(value * scale)
    }

    pub fn scale_for_rem(rem_size: Pixels) -> f32 {
        rem_size / px(BASE_REM_SIZE)
    }

    pub fn px_for_rem(value: f32, rem_size: Pixels) -> Pixels {
        px_for_scale(value, scale_for_rem(rem_size))
    }
}
