//! Presentation snapshots for one proposed prompt rewrite. No document mutation lives here.
use std::{
    ops::Range,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    div, list, prelude::*, px, rgb, Context, HighlightStyle, ListAlignment, ListState, SharedString, StyledText,
};
use lst_editor::TabId;
use similar::{ChangeTag, TextDiff};

use crate::{
    ui::theme::{metrics, typography, Theme},
    LstGpuiApp,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Context,
    Unchanged,
    Inline,
    Before,
    After,
}

#[derive(Clone, Debug)]
pub(crate) struct Mark {
    pub range: Range<usize>,
    pub removed: bool,
}

#[derive(Debug)]
pub(crate) struct Row {
    pub kind: Kind,
    pub text: String,
    pub highlights: Vec<Mark>,
}

pub(crate) struct PreparedReview {
    pub source: String,
    pub result: String,
    pub warning: String,
    pub changes: Vec<Row>,
    pub clean: Vec<Row>,
}

impl PreparedReview {
    pub fn new(source: String, result: String, warning: String) -> Self {
        // Both passes share one deadline. On difficult inputs similar returns a coarser,
        // still exact comparison. Work is performed on the background executor.
        let deadline = Instant::now() + Duration::from_millis(150);
        let diff = TextDiff::configure().deadline(deadline).diff_lines(&source, &result);
        let mut changes = Vec::new();
        let mut before = String::new();
        let mut after = String::new();
        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Delete => before.push_str(change.value()),
                ChangeTag::Insert => after.push_str(change.value()),
                ChangeTag::Equal => {
                    changed_rows(&mut changes, &before, &after, deadline);
                    before.clear();
                    after.clear();
                    push_rows(&mut changes, Kind::Unchanged, change.value(), &[]);
                }
            }
        }
        changed_rows(&mut changes, &before, &after, deadline);
        let mut clean = Vec::new();
        push_rows(&mut clean, Kind::Unchanged, &result, &[]);
        Self {
            source,
            result,
            warning,
            changes,
            clean,
        }
    }

    pub fn with_context(mut self, before: &str, after: &str) -> Self {
        for rows in [&mut self.changes, &mut self.clean] {
            let mut context = Vec::new();
            push_rows(&mut context, Kind::Context, before, &[]);
            rows.splice(0..0, context);
            push_rows(rows, Kind::Context, after, &[]);
        }
        self
    }
}

fn changed_rows(rows: &mut Vec<Row>, before: &str, after: &str, deadline: Instant) {
    if before.is_empty() && after.is_empty() {
        return;
    }
    let diff = TextDiff::configure()
        .deadline(deadline)
        .diff_unicode_words(before, after);
    let (mut old_offset, mut new_offset) = (0, 0);
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let mut inline = String::new();
    let mut inline_marks = Vec::new();
    let (mut retained_words, mut old_words, mut new_words, mut edit_groups) = (0usize, 0usize, 0usize, 0usize);
    let mut editing = false;
    let mut structural = false;
    for change in diff.iter_all_changes() {
        let value = change.value();
        let len = value.len();
        let word = usize::from(value.chars().any(char::is_alphanumeric));
        let offset = inline.len();
        inline.push_str(value);
        match change.tag() {
            ChangeTag::Equal => {
                old_offset += len;
                new_offset += len;
                old_words += word;
                new_words += word;
                retained_words += word;
                if word > 0 {
                    editing = false;
                }
            }
            tag => {
                if !editing {
                    edit_groups += 1;
                    editing = true;
                }
                structural |= value.contains(['\r', '\n']);
                let deleted = tag == ChangeTag::Delete;
                inline_marks.push(Mark {
                    range: offset..offset + len,
                    removed: deleted,
                });
                if deleted {
                    removed.push(old_offset..old_offset + len);
                    old_offset += len;
                    old_words += word;
                } else {
                    added.push(new_offset..new_offset + len);
                    new_offset += len;
                    new_words += word;
                }
            }
        }
    }
    // Stable per-passage rule: localized wording changes stay in the prose flow.
    // Structural edits and dense rewrites use a compact, readable pair instead.
    let light_edit = !before.is_empty()
        && !after.is_empty()
        && !structural
        && edit_groups <= 4
        && retained_words * 5 >= old_words.max(new_words).max(1) * 3;
    if light_edit {
        push_rows(rows, Kind::Inline, &inline, &inline_marks);
    } else {
        let removed = phrase_highlights(before, removed)
            .into_iter()
            .map(|range| Mark { range, removed: true })
            .collect::<Vec<_>>();
        let added = phrase_highlights(after, added)
            .into_iter()
            .map(|range| Mark { range, removed: false })
            .collect::<Vec<_>>();
        push_rows(rows, Kind::Before, before, &removed);
        push_rows(rows, Kind::After, after, &added);
    }
}

fn phrase_highlights(text: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut() {
            if text[previous.end..range.start].chars().all(char::is_whitespace) {
                previous.end = range.end;
                continue;
            }
        }
        merged.push(range);
    }
    merged
}

// Bound each shaped item as well as virtualizing the list. A long unbroken paragraph
// must not require shaping the entire document on every frame. These are byte ranges.
fn push_rows(rows: &mut Vec<Row>, kind: Kind, text: &str, highlights: &[Mark]) {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let mut rest = line;
        while !rest.is_empty() {
            let mut end = rest.len().min(4096);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            if end < rest.len() {
                if let Some((boundary, _)) = rest[..end].char_indices().rev().find(|(_, c)| c.is_whitespace()) {
                    if boundary > 2048 {
                        end = boundary + rest[boundary..].chars().next().unwrap().len_utf8();
                    }
                }
            }
            let ranges = highlights
                .iter()
                .filter_map(|mark| {
                    let start = mark.range.start.max(offset);
                    let end = mark.range.end.min(offset + end);
                    (start < end).then_some(Mark {
                        range: start.saturating_sub(offset)..end.saturating_sub(offset),
                        removed: mark.removed,
                    })
                })
                .collect();
            rows.push(Row {
                kind,
                text: rest[..end].to_string(),
                highlights: ranges,
            });
            rest = &rest[end..];
            offset += end;
        }
    }
}

pub(crate) struct PromptReview {
    pub tab_id: TabId,
    pub revision: u64,
    pub range: Range<usize>,
    pub selection: bool,
    pub prepared: Rc<PreparedReview>,
    pub show_result: bool,
    changes_scroll: ListState,
    result_scroll: ListState,
    layout_key: Option<(gpui::Font, u32, u32)>,
}

impl PromptReview {
    pub fn new(tab_id: TabId, revision: u64, range: Range<usize>, selection: bool, prepared: PreparedReview) -> Self {
        let changes_scroll = ListState::new(prepared.changes.len(), ListAlignment::Top, px(300.0));
        let result_scroll = ListState::new(prepared.clean.len(), ListAlignment::Top, px(300.0));
        Self {
            tab_id,
            revision,
            range,
            selection,
            prepared: Rc::new(prepared),
            show_result: false,
            changes_scroll,
            result_scroll,
            layout_key: None,
        }
    }
}

impl LstGpuiApp {
    pub(crate) fn toggle_prompt_review_view(&mut self, cx: &mut Context<Self>) {
        if let Some(review) = self.prompt_review.as_mut() {
            review.show_result = !review.show_result;
            cx.notify();
        }
    }

    pub(crate) fn scroll_prompt_review(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(review) = self.prompt_review.as_ref() else {
            return;
        };
        let (scroll, count) = if review.show_result {
            (&review.result_scroll, review.prepared.clean.len())
        } else {
            (&review.changes_scroll, review.prepared.changes.len())
        };
        match key {
            "home" => scroll.scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: px(0.0),
            }),
            "end" => scroll.scroll_to_reveal_item(count.saturating_sub(1)),
            "up" => scroll.scroll_by(px(-48.0)),
            "down" => scroll.scroll_by(px(48.0)),
            "pageup" => scroll.scroll_by(px(-400.0)),
            "pagedown" => scroll.scroll_by(px(400.0)),
            _ => return,
        }
        cx.notify();
    }

    pub(crate) fn render_prompt_review(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let review = self.prompt_review.as_mut().expect("review is open");
        let layout_key = (
            typography::primary_font(),
            metrics::code_font_size().to_bits(),
            scale.to_bits(),
        );
        if review.layout_key.as_ref().is_some_and(|key| key != &layout_key) {
            review.changes_scroll.reset(review.prepared.changes.len());
            review.result_scroll.reset(review.prepared.clean.len());
        }
        review.layout_key = Some(layout_key);
        let prepared = review.prepared.clone();
        let show_result = review.show_result;
        let selection = review.selection;
        let scroll = if show_result {
            review.result_scroll.clone()
        } else {
            review.changes_scroll.clone()
        };
        let stale = self
            .model
            .tab_by_id(review.tab_id)
            .is_none_or(|tab| tab.revision() != review.revision)
            || self.model.active_tab_id() != review.tab_id;
        let unchanged = prepared.source == prepared.result;
        let footer = if stale {
            "The document changed. Discard this review and polish again.".to_string()
        } else if !prepared.warning.is_empty() {
            prepared.warning.clone()
        } else if unchanged {
            "No changes. Your original text is already up to date.".to_string()
        } else {
            "Your original stays untouched until you apply. You can undo after applying.".to_string()
        };
        let newline_note = if prepared.source.ends_with('\n') != prepared.result.ends_with('\n') {
            if prepared.result.ends_with('\n') {
                " · Final newline added"
            } else {
                " · Final newline removed"
            }
        } else {
            ""
        };
        div()
            .id("prompt-review")
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .bg(rgb(theme.role.editor_bg))
            .occlude()
            .child(
                div()
                    .flex_none()
                    .px_3()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgb(theme.role.panel_bg))
                    .border_b_1()
                    .border_color(rgb(theme.role.border))
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text_subtle))
                            .child(if selection { "Review selection" } else { "Review prompt" }),
                    )
                    .child(
                        review_button("review-changes", "Changes", !show_result, true, theme, scale).on_click(
                            cx.listener(|this, _, _, cx| {
                                if this.prompt_review.as_ref().is_some_and(|r| r.show_result) {
                                    this.toggle_prompt_review_view(cx);
                                }
                            }),
                        ),
                    )
                    .child(
                        review_button("review-result", "Result", show_result, true, theme, scale).on_click(
                            cx.listener(|this, _, _, cx| {
                                if this.prompt_review.as_ref().is_some_and(|r| !r.show_result) {
                                    this.toggle_prompt_review_view(cx);
                                }
                            }),
                        ),
                    )
                    .child(div().flex_1())
                    .child(
                        review_button("review-discard", "Discard (Esc)", false, true, theme, scale)
                            .on_click(cx.listener(|this, _, _, cx| this.discard_prompt_review(cx))),
                    )
                    .child(
                        review_button("review-apply", "Apply (Enter)", true, !stale, theme, scale)
                            .when(!stale, |button| {
                                button.on_click(cx.listener(|this, _, _, cx| this.apply_prompt_review(cx)))
                            }),
                    ),
            )
            .when(
                stale || unchanged || !review.prepared.warning.is_empty() || !newline_note.is_empty(),
                |view| {
                    view.child(
                        div()
                            .flex_none()
                            .px_3()
                            .py_1()
                            .whitespace_normal()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(if stale {
                                theme.role.error_text
                            } else {
                                theme.role.text_subtle
                            }))
                            .child(format!("{footer}{newline_note}")),
                    )
                },
            )
            .child(
                div().flex_1().min_h_0().child(
                    list(scroll, move |index, _, _| {
                        let rows = if show_result {
                            &prepared.clean
                        } else {
                            &prepared.changes
                        };
                        render_row(&rows[index], theme, scale).into_any_element()
                    })
                    .size_full(),
                ),
            )
    }
}

fn review_button(
    id: &'static str,
    label: &'static str,
    active: bool,
    enabled: bool,
    theme: Theme,
    scale: f32,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .px_3()
        .py_1()
        .rounded_sm()
        .text_size(metrics::px_for_scale(13.0, scale))
        .bg(rgb(if active && enabled {
            theme.role.accent
        } else {
            theme.role.control_bg
        }))
        .text_color(rgb(if !enabled {
            theme.role.text_muted
        } else if active {
            theme.role.accent_text
        } else {
            theme.role.text
        }))
        .when(enabled, |button| {
            button
                .cursor(gpui::CursorStyle::PointingHand)
                .hover(|s| s.opacity(0.85))
        })
        .child(label)
}

fn render_row(row: &Row, theme: Theme, scale: f32) -> impl IntoElement {
    let label = match row.kind {
        Kind::Before => "−",
        Kind::After => "+",
        Kind::Context => "⋮",
        _ => "",
    };
    let color = match row.kind {
        Kind::Before => theme.role.diff_removed_text,
        Kind::After => theme.role.diff_added_text,
        _ => theme.role.text_muted,
    };
    let whitespace_only =
        !row.text.is_empty() && row.text.trim().is_empty() && matches!(row.kind, Kind::Before | Kind::After);
    let text: SharedString = if whitespace_only {
        row.text
            .replace(' ', "·")
            .replace('\t', "→")
            .replace('\r', "␍")
            .replace('\n', "↵")
            .into()
    } else {
        row.text.trim_end_matches(['\r', '\n']).to_string().into()
    };
    let highlights = if whitespace_only {
        vec![]
    } else {
        row.highlights
            .iter()
            .filter_map(|mark| {
                let range = mark.range.start.min(text.len())..mark.range.end.min(text.len());
                let (foreground, background) = if mark.removed {
                    (theme.role.diff_removed_text, theme.role.diff_removed_bg)
                } else {
                    (theme.role.diff_added_text, theme.role.diff_added_bg)
                };
                (!range.is_empty()).then_some((
                    range,
                    HighlightStyle {
                        background_color: Some(rgb(background).into()),
                        color: Some(rgb(foreground).into()),
                        strikethrough: (mark.removed && row.kind == Kind::Inline).then_some(gpui::StrikethroughStyle {
                            thickness: px(1.0),
                            color: Some(rgb(foreground).into()),
                        }),
                        ..Default::default()
                    },
                ))
            })
            .collect::<Vec<_>>()
    };
    div()
        .w_full()
        .flex()
        .items_start()
        .child(
            div()
                .flex_none()
                .w(metrics::px_for_scale(36.0, scale))
                .text_center()
                .text_size(metrics::px_for_scale(11.0, scale))
                .line_height(metrics::px_for_scale(metrics::row_height(), scale))
                .text_color(rgb(color))
                .child(label),
        )
        .child(
            div()
                .w_0()
                .flex_1()
                .min_w_0()
                .pr_3()
                .whitespace_normal()
                .font(typography::primary_font())
                .text_size(metrics::px_for_scale(metrics::code_font_size(), scale))
                .line_height(metrics::px_for_scale(metrics::row_height(), scale))
                .text_color(rgb(if row.kind == Kind::Context {
                    theme.role.text_muted
                } else {
                    theme.role.text
                }))
                .child(StyledText::new(text).with_highlights(highlights)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_preserves_both_texts_and_unicode_boundaries() {
        for (old, new) in [
            ("café 🐈\n\nold\n", "café 🐕\n\nnew"),
            ("", "added"),
            ("removed", ""),
            ("same", "same"),
            ("a \r\n", "a\n"),
            (
                "Please, um, fix the search panel today.",
                "Please fix the search panel today.",
            ),
            (
                "Keep the old parser and all its tests.",
                "Keep the new parser and all its tests.",
            ),
        ] {
            let review = PreparedReview::new(old.into(), new.into(), String::new());
            for (kind, expected) in [(Kind::After, old), (Kind::Before, new)] {
                let actual: String = review
                    .changes
                    .iter()
                    .filter(|r| r.kind != kind)
                    .map(|r| {
                        if r.kind != Kind::Inline {
                            return r.text.clone();
                        }
                        let mut text = String::new();
                        let mut offset = 0;
                        for mark in &r.highlights {
                            if mark.removed == (kind == Kind::Before) {
                                text.push_str(&r.text[offset..mark.range.start]);
                                offset = mark.range.end;
                            }
                        }
                        text.push_str(&r.text[offset..]);
                        text
                    })
                    .collect();
                assert_eq!(actual, expected);
            }
            for row in review.changes {
                for range in row.highlights {
                    assert!(row.text.get(range.range).is_some());
                }
            }
        }
    }
    #[test]
    fn small_corrections_stay_inline_and_rewrites_get_separate_passages() {
        let small = PreparedReview::new(
            "Please, um, review the search panel today.".into(),
            "Please review the search panel today.".into(),
            String::new(),
        );
        assert!(small.changes.iter().all(|row| row.kind == Kind::Inline));
        assert!(small
            .changes
            .iter()
            .flat_map(|row| &row.highlights)
            .any(|mark| mark.removed));
        let large = PreparedReview::new(
            "It is broken, can you help?".into(),
            "Investigate the failure and verify the fix.".into(),
            String::new(),
        );
        assert!(large.changes.iter().any(|row| row.kind == Kind::Before));
        assert!(large.changes.iter().any(|row| row.kind == Kind::After));
        assert!(!large.changes.iter().any(|row| row.kind == Kind::Inline));
    }

    #[test]
    fn long_paragraphs_are_bounded_without_losing_text() {
        let source = "é🦀".repeat(10_000);
        let review = PreparedReview::new(source.clone(), format!("{source} changed"), String::new());
        assert!(review.changes.iter().all(|r| r.text.len() <= 4096));
        assert_eq!(
            review.clean.iter().map(|r| r.text.as_str()).collect::<String>(),
            review.result
        );
    }
}
