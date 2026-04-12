pub mod palette {
    pub const BASE: u32 = 0x11111B;
    pub const SURFACE_LOW: u32 = 0x181825;
    pub const SURFACE: u32 = 0x1E1E2E;
    pub const SURFACE_HIGH: u32 = 0x313244;
    pub const OVERLAY: u32 = 0x45475A;
    pub const TEXT: u32 = 0xCDD6F4;
    pub const SUBTEXT: u32 = 0xA6ADC8;
    pub const MUTED: u32 = 0x6C7086;
    pub const BLUE: u32 = 0x89B4FA;
    pub const GREEN: u32 = 0xA6E3A1;
    pub const YELLOW: u32 = 0xF9E2AF;
    pub const PEACH: u32 = 0xFAB387;
    pub const PINK: u32 = 0xF5C2E7;
    pub const MAUVE: u32 = 0xCBA6F7;
    pub const SAPPHIRE: u32 = 0x74C7EC;
    pub const LAVENDER: u32 = 0xB4BEFE;
    pub const ROSEWATER: u32 = 0xF5E0DC;
}

pub mod role {
    use super::palette;

    pub const APP_BG: u32 = palette::BASE;
    pub const PANEL_BG: u32 = palette::SURFACE_LOW;
    pub const EDITOR_BG: u32 = palette::SURFACE;
    pub const CONTROL_BG: u32 = palette::SURFACE;
    pub const CONTROL_BG_HOVER: u32 = palette::SURFACE_HIGH;
    pub const BORDER: u32 = palette::OVERLAY;
    pub const TEXT: u32 = palette::TEXT;
    pub const TEXT_SUBTLE: u32 = palette::SUBTEXT;
    pub const TEXT_MUTED: u32 = palette::MUTED;
    pub const ACCENT: u32 = palette::BLUE;
    pub const DIRTY: u32 = palette::PEACH;
    pub const SELECTION_BG: u32 = 0x585B70;
    pub const CARET: u32 = palette::ROSEWATER;
    pub const CURRENT_LINE_BG: u32 = 0x181B2B;
    pub const GUTTER_BG: u32 = 0x161622;
}

pub mod syntax {
    use super::{palette, role};

    pub const COMMENT: u32 = role::TEXT_MUTED;
    pub const STRING: u32 = palette::GREEN;
    pub const CONSTANT: u32 = palette::PEACH;
    pub const FUNCTION: u32 = role::ACCENT;
    pub const KEYWORD: u32 = palette::MAUVE;
    pub const OPERATOR: u32 = palette::SAPPHIRE;
    pub const TYPE: u32 = palette::YELLOW;
    pub const TAG: u32 = palette::SAPPHIRE;
    pub const TITLE: u32 = palette::YELLOW;
    pub const STRONG: u32 = palette::PEACH;
    pub const EMPHASIS: u32 = palette::PINK;
    pub const LITERAL: u32 = palette::GREEN;
    pub const REFERENCE: u32 = palette::SAPPHIRE;
    pub const PROPERTY: u32 = palette::LAVENDER;
    pub const ESCAPE: u32 = palette::PINK;
    pub const PUNCTUATION: u32 = role::BORDER;
    pub const LABEL: u32 = palette::LAVENDER;
}

pub mod metrics {
    pub const WINDOW_WIDTH: f32 = 1360.0;
    pub const WINDOW_HEIGHT: f32 = 860.0;
    pub const SHELL_EDGE_PAD: f32 = 12.0;
    pub const SHELL_GAP: f32 = 8.0;
    pub const STATUS_HEIGHT_PAD: f32 = 10.0;

    pub const TAB_HEIGHT: f32 = 30.0;
    pub const TAB_MIN_WIDTH: f32 = 128.0;
    pub const TAB_MAX_WIDTH: f32 = 220.0;
    pub const TAB_HORIZONTAL_PAD: f32 = 10.0;
    pub const TAB_SLOT_WIDTH: f32 = 18.0;
    pub const TAB_TEXT_SIZE: f32 = 12.0;
    pub const ICON_BUTTON_SIZE: f32 = 16.0;

    pub const INPUT_HEIGHT: f32 = 30.0;
    pub const INPUT_TEXT_SIZE: f32 = 12.0;

    pub const ROW_HEIGHT: f32 = 22.0;
    pub const GUTTER_WIDTH: f32 = 76.0;
    pub const CODE_FONT_SIZE: f32 = 13.0;
    pub const CURSOR_WIDTH: f32 = 2.0;
    pub const VIEWPORT_OVERSCAN_LINES: usize = 6;
    pub const EDITOR_LEFT_PAD: f32 = 18.0;
    pub const EDITOR_RIGHT_PAD: f32 = 28.0;
    pub const GUTTER_LEFT_PAD: f32 = 12.0;
    pub const GUTTER_SEPARATOR_WIDTH: f32 = 14.0;
    pub const WRAP_CHAR_WIDTH_FALLBACK: f32 = 7.8;
}
