use crate::{
    document::{char_to_position, position_to_char},
    selection::{next_grapheme_boundary, previous_grapheme_boundary, CursorGoal, Position, Selection, SelectionState, SelectionTransform},
    tab::EditorTab,
    wrap,
};

#[rustfmt::skip]
pub(crate) fn horizontal(tab: &EditorTab, delta: isize, select: bool) -> Option<SelectionState> {
    map(tab, |_, selection| {
        let mut target = collapsed_edge(selection, delta.is_negative(), select).unwrap_or(selection.cursor());
        for _ in 0..delta.unsigned_abs() {
            let next = if delta.is_negative() { previous_grapheme_boundary(tab.buffer(), target) } else { next_grapheme_boundary(tab.buffer(), target) };
            if next == target { break; }
            target = next;
        }
        SelectionTransform::new(selection_to(selection, target, select))
    })
}

#[rustfmt::skip]
pub(crate) fn horizontal_collapsed(tab: &EditorTab, backward: bool) -> Option<SelectionState> {
    let range = tab.selected_range();
    if range.start != range.end {
        return Some(SelectionState::single(Selection::collapsed(if backward { range.start } else { range.end })));
    }
    horizontal(tab, if backward { -1 } else { 1 }, false)
}

#[rustfmt::skip]
pub(crate) fn boundary(
    tab: &EditorTab,
    backward: bool,
    select: bool,
    prev_fn: fn(&ropey::Rope, usize) -> usize,
    next_fn: fn(&ropey::Rope, usize) -> usize,
) -> Option<SelectionState> {
    map(tab, |_, selection| {
        let target = collapsed_edge(selection, backward, select).unwrap_or_else(|| {
            if backward { prev_fn(tab.buffer(), selection.cursor()) } else { next_fn(tab.buffer(), selection.cursor()) }
        });
        SelectionTransform::new(selection_to(selection, target, select))
    })
}

#[rustfmt::skip]
pub(crate) fn vertical(tab: &EditorTab, delta: isize, select: bool, snap: bool) -> Option<SelectionState> {
    map(tab, |index, selection| {
        let goal = goal_for(tab, index, selection);
        let target = vertical_target(tab, selection.cursor(), goal, delta, snap);
        transform_with_goal(tab, selection, target, select, goal)
    })
}

#[rustfmt::skip]
pub(crate) fn display_rows(
    tab: &EditorTab,
    lines: Option<&[String]>,
    show_wrap: bool,
    delta: isize,
    select: bool,
    wrap_columns: usize,
    snap: bool,
) -> Option<SelectionState> {
    if !show_wrap { return vertical(tab, delta, select, snap); }
    let lines = lines?;
    let layout = wrap::build_wrap_layout(lines, wrap_columns, true);
    map(tab, |index, selection| {
        let position = char_to_position(tab.buffer(), selection.cursor());
        let goal = goal_for(tab, index, selection);
        let preferred = display_preferred(tab, lines, &layout, position, goal);
        let row_target = wrap::display_row_target(lines, position.line, position.column, Some(preferred), delta, &layout);
        let target = row_target
            .map(|target| position_to_char(tab.buffer(), Position::new(target.line, target.column)))
            .or_else(|| snap.then(|| vertical_boundary_target(tab, delta)).flatten())
            .unwrap_or(selection.cursor());
        let goal = if tab.selection_set().has_multiple() { goal } else { CursorGoal::Column(preferred) };
        transform_with_goal(tab, selection, target, select, goal)
    })
}

#[rustfmt::skip]
pub(crate) fn visual_row(
    tab: &EditorTab,
    lines: Option<&[String]>,
    show_wrap: bool,
    target_row: usize,
    select: bool,
    wrap_columns: usize,
) -> Option<SelectionState> {
    if !show_wrap {
        let current = tab.cursor_position().line;
        return (target_row != current).then(|| vertical(tab, target_row as isize - current as isize, select, true)).flatten();
    }
    let lines = lines?;
    let position = tab.cursor_position();
    let layout = wrap::build_wrap_layout(lines, wrap_columns, true);
    let current_row = wrap::visual_row_for_position(lines, position.line, position.column, &layout).unwrap_or(position.line);
    let row_target = (target_row != current_row)
        .then(|| wrap::display_row_target(lines, position.line, position.column, tab.preferred_column(), target_row as isize - current_row as isize, &layout))??;
    let selection = selection_to(
        tab.selection(),
        position_to_char(tab.buffer(), Position::new(row_target.line, row_target.column)),
        select,
    );
    Some(SelectionState::single_with_transform(SelectionTransform::with_columns(
        selection,
        CursorGoal::Column(row_target.preferred_column),
        (!selection.has_selection()).then_some(row_target.preferred_column),
    )))
}

#[rustfmt::skip]
pub(crate) fn line_boundary(tab: &EditorTab, to_end: bool, select: bool) -> Option<SelectionState> {
    map(tab, |_, selection| {
        let line = tab.buffer().char_to_line(selection.cursor().min(tab.len_chars()));
        let target = tab.buffer().line_to_char(line) + if to_end { display_line_char_len(tab, line) } else { 0 };
        SelectionTransform::new(selection_to(selection, target, select))
    })
}

#[rustfmt::skip]
pub(crate) fn smart_home(tab: &EditorTab, select: bool) -> Option<SelectionState> {
    map(tab, |_, selection| {
        let cursor = selection.cursor();
        let line = tab.buffer().char_to_line(cursor.min(tab.len_chars()));
        let line_start = tab.buffer().line_to_char(line);
        let first_non_blank = line_start + first_non_blank_column(tab, line);
        SelectionTransform::new(selection_to(selection, if cursor == first_non_blank { line_start } else { first_non_blank }, select))
    })
}

#[rustfmt::skip]
pub(crate) fn document_boundary(tab: &EditorTab, to_end: bool, select: bool) -> Option<SelectionState> {
    let target = if to_end { tab.len_chars() } else { 0 };
    map(tab, |_, selection| SelectionTransform::new(selection_to(selection, target, select)))
}

#[rustfmt::skip]
fn map<F>(tab: &EditorTab, f: F) -> Option<SelectionState>
where
    F: FnMut(usize, Selection) -> SelectionTransform,
{
    let before = tab.selection_state();
    let after = before.map(f)?;
    (after != *before).then_some(after)
}

#[rustfmt::skip]
fn collapsed_edge(selection: Selection, backward: bool, select: bool) -> Option<usize> {
    (!select && selection.has_selection()).then(|| {
        let range = selection.range();
        if backward { range.start } else { range.end }
    })
}

#[rustfmt::skip]
fn selection_to(selection: Selection, target: usize, select: bool) -> Selection {
    if select { Selection::new(selection.anchor(), target) } else { Selection::collapsed(target) }
}

#[rustfmt::skip]
fn goal_for(tab: &EditorTab, index: usize, selection: Selection) -> CursorGoal {
    let position = char_to_position(tab.buffer(), selection.cursor());
    if tab.selection_set().has_multiple() {
        tab.preferred_goal_for_selection(index).unwrap_or(CursorGoal::Column(position.column))
    } else {
        CursorGoal::Column(tab.preferred_column().unwrap_or(position.column))
    }
}

#[rustfmt::skip]
fn vertical_target(tab: &EditorTab, cursor: usize, goal: CursorGoal, delta: isize, snap: bool) -> usize {
    let position = char_to_position(tab.buffer(), cursor);
    let last_line = tab.line_count().saturating_sub(1);
    if snap && ((delta < 0 && position.line == 0) || (delta > 0 && position.line == last_line)) {
        if let Some(target) = vertical_boundary_target(tab, delta) { return target; }
    }
    let target_line = if delta.is_negative() { position.line.saturating_sub(delta.unsigned_abs()) } else { (position.line + delta as usize).min(last_line) };
    tab.buffer().line_to_char(target_line) + goal.resolve(display_line_char_len(tab, target_line))
}

#[rustfmt::skip]
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

#[rustfmt::skip]
fn transform_with_goal(tab: &EditorTab, selection: Selection, target: usize, select: bool, goal: CursorGoal) -> SelectionTransform {
    let selection = selection_to(selection, target, select);
    let actual_column = char_to_position(tab.buffer(), target).column;
    let visible_column = (!selection.has_selection()).then_some(match goal { CursorGoal::Column(column) => column, CursorGoal::LineEnd => actual_column });
    SelectionTransform::with_columns(selection, goal, visible_column)
}

#[rustfmt::skip]
fn display_preferred(tab: &EditorTab, lines: &[String], layout: &wrap::WrapLayout, position: Position, goal: CursorGoal) -> usize {
    if !tab.selection_set().has_multiple() && tab.preferred_column().is_none() {
        return current_visual_column(lines, layout, position);
    }
    match goal { CursorGoal::Column(column) => column, CursorGoal::LineEnd => display_line_char_len(tab, position.line) }
}

#[rustfmt::skip]
fn current_visual_column(lines: &[String], layout: &wrap::WrapLayout, position: Position) -> usize {
    let current_visual_row = wrap::visual_row_for_position(lines, position.line, position.column, layout).unwrap_or(layout.line_row_starts[position.line]);
    let row_in_line = current_visual_row.saturating_sub(layout.line_row_starts[position.line]);
    let current_line = lines.get(position.line).map(String::as_str).unwrap_or_default();
    let segments = wrap::wrap_segments(current_line, layout.wrap_columns);
    let current_segment = segments.get(row_in_line).or_else(|| segments.last()).expect("wrap_segments always returns at least one segment");
    position.column.saturating_sub(current_segment.start_col)
}

fn display_line_char_len(tab: &EditorTab, line_ix: usize) -> usize {
    crate::selection::display_line_char_len(tab.buffer(), line_ix)
}

fn first_non_blank_column(tab: &EditorTab, line_ix: usize) -> usize {
    tab.buffer().line(line_ix.min(tab.buffer().len_lines().saturating_sub(1))).chars().take_while(|ch| *ch != '\n' && *ch != '\r').position(|ch| !ch.is_whitespace()).unwrap_or(0)
}
