use gpui::{font, px, App, Font, FontFallbacks, Global, Pixels};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum ThemeId {
    #[default]
    Dark,
    Light,
}

impl Global for ThemeId {}

impl ThemeId {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    pub(crate) fn theme(self) -> Theme {
        match self {
            Self::Dark => DARK,
            Self::Light => LIGHT,
        }
    }
}

/// The active theme, sourced from the single `ThemeId` global the app
/// installs at startup. Render code should read through this rather than
/// holding a per-widget copy that has to be re-synced on theme changes.
pub(crate) fn current_theme(cx: &App) -> Theme {
    cx.global::<ThemeId>().theme()
}

pub(crate) fn current_theme_id(cx: &App) -> ThemeId {
    *cx.global::<ThemeId>()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Theme {
    pub(crate) id: ThemeId,
    pub(crate) name: &'static str,
    pub(crate) role: RoleColors,
    pub(crate) syntax: SyntaxColors,
}

impl Theme {
    pub(crate) fn style_key(self) -> u64 {
        match self.id {
            ThemeId::Dark => 1,
            ThemeId::Light => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RoleColors {
    pub(crate) app_bg: u32,
    pub(crate) panel_bg: u32,
    pub(crate) editor_bg: u32,
    pub(crate) control_bg: u32,
    pub(crate) control_bg_hover: u32,
    pub(crate) border: u32,
    pub(crate) text: u32,
    pub(crate) text_subtle: u32,
    pub(crate) text_muted: u32,
    pub(crate) accent: u32,
    pub(crate) accent_text: u32,
    pub(crate) error_text: u32,
    pub(crate) selection_bg: u32,
    pub(crate) search_match_bg: u32,
    pub(crate) search_active_match_bg: u32,
    pub(crate) caret: u32,
    pub(crate) current_line_bg: u32,
    pub(crate) gutter_bg: u32,
    pub(crate) scrollbar_thumb: u32,
    pub(crate) scrollbar_thumb_active: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SyntaxRole {
    Comment,
    String,
    Constant,
    Function,
    Keyword,
    Operator,
    Type,
    Tag,
    Title,
    Strong,
    Emphasis,
    Literal,
    Reference,
    Property,
    Escape,
    Punctuation,
    Label,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SyntaxColors {
    pub(crate) comment: u32,
    pub(crate) string: u32,
    pub(crate) constant: u32,
    pub(crate) function: u32,
    pub(crate) keyword: u32,
    pub(crate) operator: u32,
    pub(crate) ty: u32,
    pub(crate) tag: u32,
    pub(crate) title: u32,
    pub(crate) strong: u32,
    pub(crate) emphasis: u32,
    pub(crate) literal: u32,
    pub(crate) reference: u32,
    pub(crate) property: u32,
    pub(crate) escape: u32,
    pub(crate) punctuation: u32,
    pub(crate) label: u32,
}

impl SyntaxColors {
    pub(crate) fn color(self, role: SyntaxRole) -> u32 {
        match role {
            SyntaxRole::Comment => self.comment,
            SyntaxRole::String => self.string,
            SyntaxRole::Constant => self.constant,
            SyntaxRole::Function => self.function,
            SyntaxRole::Keyword => self.keyword,
            SyntaxRole::Operator => self.operator,
            SyntaxRole::Type => self.ty,
            SyntaxRole::Tag => self.tag,
            SyntaxRole::Title => self.title,
            SyntaxRole::Strong => self.strong,
            SyntaxRole::Emphasis => self.emphasis,
            SyntaxRole::Literal => self.literal,
            SyntaxRole::Reference => self.reference,
            SyntaxRole::Property => self.property,
            SyntaxRole::Escape => self.escape,
            SyntaxRole::Punctuation => self.punctuation,
            SyntaxRole::Label => self.label,
        }
    }
}

const DARK: Theme = Theme {
    id: ThemeId::Dark,
    name: "Dark",
    role: RoleColors {
        app_bg: 0x181818,
        panel_bg: 0x252526,
        editor_bg: 0x1F1F1F,
        control_bg: 0x313131,
        control_bg_hover: 0x3C3C3C,
        border: 0x3C3C3C,
        text: 0xCCCCCC,
        text_subtle: 0xA6A6A6,
        text_muted: 0x808080,
        accent: 0x0078D4,
        accent_text: 0xFFFFFF,
        error_text: 0xF14C4C,
        selection_bg: 0x264F78,
        search_match_bg: 0x3A3D41,
        search_active_match_bg: 0x6B4F1D,
        caret: 0xCCCCCC,
        current_line_bg: 0x2A2D2E,
        gutter_bg: 0x181818,
        scrollbar_thumb: 0x5A5A5A,
        scrollbar_thumb_active: 0x808080,
    },
    syntax: SyntaxColors {
        comment: 0x6A9955,
        string: 0xCE9178,
        constant: 0xB5CEA8,
        function: 0xDCDCAA,
        keyword: 0x569CD6,
        operator: 0xCCCCCC,
        ty: 0x4EC9B0,
        tag: 0x569CD6,
        title: 0x569CD6,
        strong: 0x569CD6,
        emphasis: 0xC586C0,
        literal: 0xCE9178,
        reference: 0x9CDCFE,
        property: 0x9CDCFE,
        escape: 0xD7BA7D,
        punctuation: 0x808080,
        label: 0x9CDCFE,
    },
};

const LIGHT: Theme = Theme {
    id: ThemeId::Light,
    name: "Light",
    role: RoleColors {
        app_bg: 0xF3F3F3,
        panel_bg: 0xF8F8F8,
        editor_bg: 0xFFFFFF,
        control_bg: 0xEDEDED,
        control_bg_hover: 0xE2E2E2,
        border: 0xD0D0D0,
        text: 0x1F2328,
        text_subtle: 0x4B5563,
        text_muted: 0x6E7781,
        accent: 0x0969DA,
        accent_text: 0xFFFFFF,
        error_text: 0xCF222E,
        selection_bg: 0xADD6FF,
        search_match_bg: 0xFFF2CC,
        search_active_match_bg: 0xF4B400,
        caret: 0x1F2328,
        current_line_bg: 0xF6F8FA,
        gutter_bg: 0xF6F8FA,
        scrollbar_thumb: 0xB8B8B8,
        scrollbar_thumb_active: 0x8C8C8C,
    },
    syntax: SyntaxColors {
        comment: 0x008000,
        string: 0xA31515,
        constant: 0x098658,
        function: 0x795E26,
        keyword: 0x0000FF,
        operator: 0x1F2328,
        ty: 0x267F99,
        tag: 0x800000,
        title: 0x0000FF,
        strong: 0x0000FF,
        emphasis: 0xAF00DB,
        literal: 0xA31515,
        reference: 0x001080,
        property: 0x001080,
        escape: 0x811F3F,
        punctuation: 0x6E7781,
        label: 0x001080,
    },
};

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
