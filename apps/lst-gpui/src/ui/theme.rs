use gpui::{font, px, Font, FontId, Pixels, TextRun, Window};

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
    pub const SELECTION_BLUE: u32 = 0x264F78;
    pub const CURRENT_LINE: u32 = 0x2A2D2E;
    pub const GUTTER: u32 = 0x181818;

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
    pub const DIRTY: u32 = palette::SYNTAX_ORANGE;
    pub const SELECTION_BG: u32 = palette::SELECTION_BLUE;
    pub const CARET: u32 = palette::TEXT;
    pub const CURRENT_LINE_BG: u32 = palette::CURRENT_LINE;
    pub const GUTTER_BG: u32 = palette::GUTTER;
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
    use super::{font, Font, FontId, Pixels, TextRun, Window};
    use std::{
        collections::{HashMap, HashSet},
        sync::OnceLock,
    };
    use unicode_segmentation::UnicodeSegmentation;

    pub const PRIMARY_FONT_FAMILY: &str = "TX-02";
    const PRIMARY_FONT_STACK: &[&str] = &[
        PRIMARY_FONT_FAMILY,
        "JetBrains Mono",
        ".ZedMono",
        "Lilex",
        "IBM Plex Mono",
    ];

    pub fn primary_font(window: &mut Window) -> Font {
        static FONT: OnceLock<Font> = OnceLock::new();

        FONT.get_or_init(|| font(selected_primary_font_families(window)[0]))
            .clone()
    }

    pub fn text_runs_with_fallbacks(
        text: &str,
        runs: &[TextRun],
        font_size: Pixels,
        window: &mut Window,
    ) -> Vec<TextRun> {
        // GPUI's Linux backend ignores Font.fallbacks, so choose concrete families before shaping.
        let fonts = selected_primary_font_families(window)
            .iter()
            .copied()
            .map(font)
            .collect::<Vec<_>>();
        if fonts.len() <= 1 {
            return runs
                .iter()
                .cloned()
                .map(|mut run| {
                    if let Some(primary) = fonts.first() {
                        run.font = primary.clone();
                    }
                    run
                })
                .collect();
        }

        let font_ids = fonts
            .iter()
            .map(|font| window.text_system().resolve_font(font))
            .collect::<Vec<_>>();
        let mut glyph_cache = HashMap::new();
        let mut split_runs = Vec::new();
        let mut offset = 0;

        for run in runs {
            let end = offset + run.len;
            let Some(run_text) = text.get(offset..end) else {
                split_runs.push(run.clone());
                offset = end;
                continue;
            };

            let mut current_font_ix = None;
            let mut current_len = 0;
            for grapheme in run_text.graphemes(true) {
                let font_ix = font_index_for_grapheme(
                    grapheme,
                    &font_ids,
                    font_size,
                    window,
                    &mut glyph_cache,
                );
                if Some(font_ix) == current_font_ix {
                    current_len += grapheme.len();
                } else {
                    push_text_run(&mut split_runs, run, current_len, current_font_ix, &fonts);
                    current_font_ix = Some(font_ix);
                    current_len = grapheme.len();
                }
            }
            push_text_run(&mut split_runs, run, current_len, current_font_ix, &fonts);
            offset = end;
        }

        split_runs
    }

    fn selected_primary_font_families(window: &mut Window) -> &'static Vec<&'static str> {
        static FAMILIES: OnceLock<Vec<&'static str>> = OnceLock::new();

        FAMILIES.get_or_init(|| {
            let available = window
                .text_system()
                .all_font_names()
                .into_iter()
                .collect::<HashSet<_>>();
            select_primary_font_families(&available)
        })
    }

    fn select_primary_font_families(available: &HashSet<String>) -> Vec<&'static str> {
        let mut selected = Vec::new();
        let mut seen_lookup_names = HashSet::new();
        for family in PRIMARY_FONT_STACK {
            let lookup = font_lookup_family(family);
            if available.contains(lookup) && seen_lookup_names.insert(lookup) {
                selected.push(*family);
            }
        }

        if selected.is_empty() {
            selected.push(PRIMARY_FONT_FAMILY);
        }
        selected
    }

    fn font_lookup_family(family: &'static str) -> &'static str {
        match family {
            ".ZedMono" => "Lilex",
            family => family,
        }
    }

    fn font_index_for_grapheme(
        grapheme: &str,
        font_ids: &[FontId],
        font_size: Pixels,
        window: &mut Window,
        glyph_cache: &mut HashMap<(usize, char), bool>,
    ) -> usize {
        for (font_ix, font_id) in font_ids.iter().enumerate() {
            if grapheme.chars().all(|ch| {
                *glyph_cache.entry((font_ix, ch)).or_insert_with(|| {
                    window
                        .text_system()
                        .advance(*font_id, font_size, ch)
                        .is_ok()
                })
            }) {
                return font_ix;
            }
        }
        0
    }

    fn push_text_run(
        runs: &mut Vec<TextRun>,
        template: &TextRun,
        len: usize,
        font_ix: Option<usize>,
        fonts: &[Font],
    ) {
        let Some(font_ix) = font_ix else {
            return;
        };
        if len == 0 {
            return;
        }

        runs.push(TextRun {
            len,
            font: fonts[font_ix].clone(),
            ..template.clone()
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn primary_font_stack_preserves_available_family_order() {
            let available =
                HashSet::from(["IBM Plex Mono".to_string(), "JetBrains Mono".to_string()]);

            assert_eq!(
                select_primary_font_families(&available),
                ["JetBrains Mono", "IBM Plex Mono"]
            );
        }

        #[test]
        fn zed_mono_alias_tracks_lilex_availability() {
            let available = HashSet::from(["Lilex".to_string(), "IBM Plex Mono".to_string()]);

            assert_eq!(
                select_primary_font_families(&available),
                [".ZedMono", "IBM Plex Mono"]
            );
        }

        #[test]
        fn missing_stack_falls_back_to_primary_family_for_gpui_resolution() {
            let available = HashSet::new();

            assert_eq!(
                select_primary_font_families(&available),
                [PRIMARY_FONT_FAMILY]
            );
        }
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
    pub const GUTTER_WIDTH: f32 = 58.0;
    pub const CODE_FONT_SIZE: f32 = 13.0;
    pub const CURSOR_WIDTH: f32 = 2.0;
    pub const VIEWPORT_OVERSCAN_LINES: usize = 6;
    pub const EDITOR_LEFT_PAD: f32 = 18.0;
    pub const EDITOR_RIGHT_PAD: f32 = 28.0;
    pub const GUTTER_LEFT_PAD: f32 = 12.0;
    pub const GUTTER_SEPARATOR_WIDTH: f32 = 14.0;
    pub const WRAP_CHAR_WIDTH_FALLBACK: f32 = 7.8;

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
