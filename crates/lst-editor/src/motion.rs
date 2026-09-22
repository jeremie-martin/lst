use crate::{
    document::{char_to_position, position_to_char},
    selection::{
        next_grapheme_boundary, previous_grapheme_boundary, CursorGoal, Position, Selection, SelectionState,
        SelectionTransform,
    },
    tab::EditorTab,
    wrap,
};
pub(crate) fn horizontal(tab: &EditorTab, delta: isize, select: bool) -> Option<SelectionState> {
    map(tab, |_, selection| {
        let mut target = collapsed_edge(selection, delta.is_negative(), select).unwrap_or(selection.cursor());
        for _ in 0..delta.unsigned_abs() {
            let next = if delta.is_negative() {
                previous_grapheme_boundary(tab.buffer(), target)
            } else {
                next_grapheme_boundary(tab.buffer(), target)
            };
            if next == target {
                break;
            }
            target = next;
        }
        SelectionTransform::new(selection_to(selection, target, select))
    })
}
pub(crate) fn horizontal_collapsed(tab: &EditorTab, backward: bool) -> Option<SelectionState> {
    let range = tab.selected_range();
    if range.start != range.end {
        return Some(SelectionState::single(Selection::collapsed(if backward {
            range.start
        } else {
            range.end
        })));
    }
    horizontal(tab, if backward { -1 } else { 1 }, false)
}
pub(crate) fn boundary(
    tab: &EditorTab,
    backward: bool,
    select: bool,
    prev_fn: fn(&ropey::Rope, usize) -> usize,
    next_fn: fn(&ropey::Rope, usize) -> usize,
) -> Option<SelectionState> {
    map(tab, |_, selection| {
        let target = collapsed_edge(selection, backward, select).unwrap_or_else(|| {
            if backward {
                prev_fn(tab.buffer(), selection.cursor())
            } else {
                next_fn(tab.buffer(), selection.cursor())
            }
        });
        SelectionTransform::new(selection_to(selection, target, select))
    })
}
pub(crate) fn vertical(tab: &EditorTab, delta: isize, select: bool, snap: bool) -> Option<SelectionState> {
    map(tab, |index, selection| {
        let goal = goal_for(tab, index, selection);
        let target = vertical_target(tab, selection.cursor(), goal, delta, snap);
        transform_with_goal(tab, selection, target, select, goal)
    })
}
pub(crate) fn display_rows(
    tab: &EditorTab,
    wrap_columns: usize,
    delta: isize,
    select: bool,
    snap: bool,
) -> Option<SelectionState> {
    map(tab, |index, selection| {
        let position = char_to_position(tab.buffer(), selection.cursor());
        let goal = goal_for(tab, index, selection);
        let preferred = display_preferred(tab, wrap_columns, position, goal);
        let row_target = wrap::display_row_target_in_rope(
            tab.buffer(),
            position.line,
            position.column,
            Some(preferred),
            delta,
            wrap_columns,
        );
        let target = row_target
            .map(|target| position_to_char(tab.buffer(), Position::new(target.line, target.column)))
            .or_else(|| snap.then(|| vertical_boundary_target(tab, delta)).flatten())
            .unwrap_or(selection.cursor());
        let goal = if tab.selection_set().has_multiple() {
            goal
        } else {
            CursorGoal::Column(preferred)
        };
        transform_with_goal(tab, selection, target, select, goal)
    })
}
/// Move both axes together, retaining the desired column across short rows.
/// Horizontal movement must not cross a row boundary and cancel the vertical step.
pub(crate) fn diagonal(tab: &EditorTab, backward: bool, rows: isize, wrap_columns: usize) -> Option<SelectionState> {
    map(tab, |index, selection| {
        let position = char_to_position(tab.buffer(), selection.cursor());
        let preferred = display_preferred(tab, wrap_columns, position, goal_for(tab, index, selection));
        let preferred = if backward {
            preferred.saturating_sub(1)
        } else {
            preferred.saturating_add(1)
        };
        let row_target = wrap::display_row_target_in_rope(
            tab.buffer(),
            position.line,
            position.column,
            Some(preferred),
            rows,
            wrap_columns,
        );
        let (line, probe_column) = row_target.map_or((position.line, position.column), |target| {
            // A positive column clamped to a wrapped row's end belongs to that
            // row, even though the shared boundary normally names the next row.
            (target.line, target.column.saturating_sub(usize::from(preferred > 0)))
        });
        let text = crate::selection::line_display_text(tab.buffer(), line);
        let segments = wrap::wrap_segments(&text, wrap_columns);
        let row = wrap::cursor_visual_row_in_line(&text, probe_column, wrap_columns).min(segments.len() - 1);
        let segment = &segments[row];
        let last_column = segment.end_col.saturating_sub(usize::from(row + 1 < segments.len()));
        let column = segment.start_col.saturating_add(preferred).min(last_column);
        let target = position_to_char(tab.buffer(), Position::new(line, column));
        let target = crate::selection::floor_grapheme_boundary(tab.buffer(), target);
        transform_with_goal(tab, selection, target, false, CursorGoal::Column(preferred))
    })
}

pub(crate) fn line_boundary(tab: &EditorTab, to_end: bool, select: bool) -> Option<SelectionState> {
    map(tab, |_, selection| {
        let line = tab.buffer().char_to_line(selection.cursor().min(tab.len_chars()));
        let target = tab.buffer().line_to_char(line) + if to_end { display_line_char_len(tab, line) } else { 0 };
        SelectionTransform::new(selection_to(selection, target, select))
    })
}
pub(crate) fn visual_line_boundary(
    tab: &EditorTab,
    wrap_columns: usize,
    show_wrap: bool,
    to_end: bool,
    select: bool,
) -> Option<SelectionState> {
    map(tab, |_, selection| {
        let position = char_to_position(tab.buffer(), selection.cursor());
        let text = crate::selection::line_display_text(tab.buffer(), position.line);
        let (segment_start, segment_end) = if show_wrap {
            let segments = wrap::wrap_segments(&text, wrap_columns);
            let mut row = wrap::cursor_visual_row_in_line(&text, position.column, wrap_columns);
            // A cursor exactly on a wrap boundary counts as the end of the
            // previous row here, or End would walk down one row per press
            // and Home on that boundary would be a permanent no-op.
            if row > 0
                && segments
                    .get(row)
                    .is_some_and(|segment| position.column == segment.start_col)
            {
                row -= 1;
            }
            let segment = segments
                .get(row)
                .or_else(|| segments.last())
                .expect("wrap_segments always returns at least one segment");
            (segment.start_col, segment.end_col)
        } else {
            (0, text.chars().count())
        };
        let target_column = if to_end {
            segment_end
        } else if segment_start > 0 {
            segment_start
        } else {
            let first_non_blank = text.chars().position(|ch| !ch.is_whitespace()).unwrap_or(0);
            if position.column == first_non_blank {
                0
            } else {
                first_non_blank
            }
        };
        let target = position_to_char(tab.buffer(), Position::new(position.line, target_column));
        SelectionTransform::new(selection_to(selection, target, select))
    })
}
pub(crate) fn smart_home(tab: &EditorTab, select: bool) -> Option<SelectionState> {
    map(tab, |_, selection| {
        let cursor = selection.cursor();
        let line = tab.buffer().char_to_line(cursor.min(tab.len_chars()));
        let line_start = tab.buffer().line_to_char(line);
        let first_non_blank = line_start + first_non_blank_column(tab, line);
        SelectionTransform::new(selection_to(
            selection,
            if cursor == first_non_blank {
                line_start
            } else {
                first_non_blank
            },
            select,
        ))
    })
}
pub(crate) fn document_boundary(tab: &EditorTab, to_end: bool, select: bool) -> Option<SelectionState> {
    let target = if to_end { tab.len_chars() } else { 0 };
    map(tab, |_, selection| {
        SelectionTransform::new(selection_to(selection, target, select))
    })
}
fn map<F>(tab: &EditorTab, f: F) -> Option<SelectionState>
where
    F: FnMut(usize, Selection) -> SelectionTransform,
{
    let before = tab.selection_state();
    let after = before.map(f)?;
    (after != *before).then_some(after)
}
fn collapsed_edge(selection: Selection, backward: bool, select: bool) -> Option<usize> {
    (!select && selection.has_selection()).then(|| {
        let range = selection.range();
        if backward {
            range.start
        } else {
            range.end
        }
    })
}
fn selection_to(selection: Selection, target: usize, select: bool) -> Selection {
    if select {
        Selection::new(selection.anchor(), target)
    } else {
        Selection::collapsed(target)
    }
}
fn goal_for(tab: &EditorTab, index: usize, selection: Selection) -> CursorGoal {
    let position = char_to_position(tab.buffer(), selection.cursor());
    if tab.selection_set().has_multiple() {
        tab.preferred_goal_for_selection(index)
            .unwrap_or(CursorGoal::Column(position.column))
    } else {
        CursorGoal::Column(tab.preferred_column().unwrap_or(position.column))
    }
}
fn vertical_target(tab: &EditorTab, cursor: usize, goal: CursorGoal, delta: isize, snap: bool) -> usize {
    let position = char_to_position(tab.buffer(), cursor);
    let last_line = tab.line_count().saturating_sub(1);
    if snap && ((delta < 0 && position.line == 0) || (delta > 0 && position.line == last_line)) {
        if let Some(target) = vertical_boundary_target(tab, delta) {
            return target;
        }
    }
    let target_line = if delta.is_negative() {
        position.line.saturating_sub(delta.unsigned_abs())
    } else {
        (position.line + delta as usize).min(last_line)
    };
    tab.buffer().line_to_char(target_line) + goal.resolve(display_line_char_len(tab, target_line))
}
fn vertical_boundary_target(tab: &EditorTab, delta: isize) -> Option<usize> {
    if delta < 0 {
        Some(tab.buffer().line_to_char(0))
    } else if delta > 0 {
        let last_line = tab.line_count().saturating_sub(1);
        Some(tab.buffer().line_to_char(last_line) + display_line_char_len(tab, last_line))
    } else {
        None
    }
}
fn transform_with_goal(
    tab: &EditorTab,
    selection: Selection,
    target: usize,
    select: bool,
    goal: CursorGoal,
) -> SelectionTransform {
    let selection = selection_to(selection, target, select);
    let actual_column = char_to_position(tab.buffer(), target).column;
    let visible_column = (!selection.has_selection()).then_some(match goal {
        CursorGoal::Column(column) => column,
        CursorGoal::LineEnd => actual_column,
    });
    SelectionTransform::with_columns(selection, goal, visible_column)
}
fn display_preferred(tab: &EditorTab, wrap_columns: usize, position: Position, goal: CursorGoal) -> usize {
    if !tab.selection_set().has_multiple() && tab.preferred_column().is_none() {
        return current_visual_column(tab.buffer(), wrap_columns, position);
    }
    match goal {
        CursorGoal::Column(column) => column,
        CursorGoal::LineEnd => display_line_char_len(tab, position.line),
    }
}
fn current_visual_column(buffer: &ropey::Rope, wrap_columns: usize, position: Position) -> usize {
    let current_line = crate::selection::line_display_text(buffer, position.line);
    let column = position.column.min(current_line.chars().count());
    let row_in_line = wrap::cursor_visual_row_in_line(&current_line, column, wrap_columns);
    let segments = wrap::wrap_segments(&current_line, wrap_columns);
    let current_segment = segments
        .get(row_in_line)
        .or_else(|| segments.last())
        .expect("wrap_segments always returns at least one segment");
    position.column.saturating_sub(current_segment.start_col)
}

fn display_line_char_len(tab: &EditorTab, line_ix: usize) -> usize {
    crate::selection::display_line_char_len(tab.buffer(), line_ix)
}

fn first_non_blank_column(tab: &EditorTab, line_ix: usize) -> usize {
    tab.buffer()
        .line(line_ix.min(tab.buffer().len_lines().saturating_sub(1)))
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .position(|ch| !ch.is_whitespace())
        .unwrap_or(0)
}

#[cfg(test)]
mod diagonal_tests {
    use super::*;
    use crate::tab::TabId;

    fn tab(text: &str, cursor: usize) -> EditorTab {
        let mut tab = EditorTab::from_path_with_stamp(TabId::from_raw(1), "diagonal.txt".into(), text, None);
        tab.move_to(cursor);
        tab
    }

    fn step(tab: &mut EditorTab, left: bool, rows: isize, columns: usize) {
        if let Some(state) = diagonal(tab, left, rows, columns) {
            tab.set_selection_state(state);
        }
    }

    #[test]
    fn column_goal_survives_empty_and_short_rows_in_every_direction() {
        let text = "abcdefghij\n\nx\n\nabcdefghij";
        for rows in [-1, 1] {
            for left in [false, true] {
                let mut tab = tab(text, if rows < 0 { 20 } else { 5 });
                for _ in 0..4 {
                    step(&mut tab, left, rows, usize::MAX);
                }
                assert_eq!(
                    tab.cursor_position(),
                    Position::new(if rows < 0 { 0 } else { 4 }, if left { 1 } else { 9 })
                );
            }
        }
    }

    #[test]
    fn wrapped_rows_clamp_horizontal_movement_without_cancelling_vertical_movement() {
        let mut tab = tab("abcdefghijklmnopqrstuvwxyz", 14);
        step(&mut tab, false, -1, 5);
        assert_eq!(tab.cursor_char(), 9);
        step(&mut tab, false, -1, 5);
        assert_eq!(tab.cursor_char(), 4);
        // At the top, the horizontal axis can still move independently.
        step(&mut tab, true, -1, 5);
        step(&mut tab, true, -1, 5);
        step(&mut tab, true, -1, 5);
        assert_eq!(tab.cursor_char(), 3);
    }

    #[test]
    fn horizontal_movement_at_document_edges_stays_on_the_row_and_respects_graphemes() {
        let mut tab = tab("e\u{301}x", 0);
        step(&mut tab, false, -1, usize::MAX);
        assert_eq!(tab.cursor_char(), 0);
        step(&mut tab, false, -1, usize::MAX);
        assert_eq!(tab.cursor_char(), 2);
        step(&mut tab, false, 1, usize::MAX);
        assert_eq!(tab.cursor_char(), 3);
    }
}
