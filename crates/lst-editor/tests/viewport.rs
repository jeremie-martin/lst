use lst_editor::{
    vim::{Key as VimKey, Modifiers as VimModifiers, NamedKey as VimNamedKey},
    EditorEffect, EditorModel, EditorTab, RevealIntent, TabId,
};

mod common;
use common::model_with_tabs;

fn long_model() -> EditorModel {
    let text = (0..200)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            &text,
        )],
        "Ready.".into(),
    );
    // Viewport tests operate on logical lines; disable soft-wrap so visual
    // rows and logical lines coincide and `wrap_columns` is irrelevant.
    model.toggle_wrap();
    let _ = model.drain_effects();
    model
}

fn only_reveal_intents(effects: &[EditorEffect]) -> Vec<RevealIntent> {
    effects
        .iter()
        .filter_map(|e| match e {
            EditorEffect::Reveal(intent) => Some(*intent),
            _ => None,
        })
        .collect()
}

#[test]
fn arrow_down_emits_reveal_nearest_edge() {
    let mut model = long_model();
    let _ = model.drain_effects();
    model.move_display_rows_by(1, false, 0);
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::NearestEdge],
    );
}

#[test]
fn horizontal_move_emits_reveal_nearest_edge() {
    let mut model = long_model();
    let _ = model.drain_effects();
    model.move_horizontal_by(1, false);
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::NearestEdge],
    );
}

#[test]
fn document_boundary_emits_reveal_nearest_edge() {
    let mut model = long_model();
    let _ = model.drain_effects();
    model.move_document_boundary(true, false);
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::NearestEdge],
    );
    model.move_document_boundary(false, false);
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::NearestEdge],
    );
}

#[test]
fn goto_line_submit_emits_reveal_center() {
    let mut model = long_model();
    model.open_goto_line_panel();
    model.update_goto_line("120".into());
    let _ = model.drain_effects();
    model.submit_goto_line_input();
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::Center],
    );
}

#[test]
fn find_query_and_next_emit_reveal_center() {
    let mut model = long_model();
    model.open_find_panel(false);
    let _ = model.drain_effects();

    model.update_find_query_and_activate("line 150".into());
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::Center],
    );

    model.update_find_query_and_activate("line 1".into());
    let _ = model.drain_effects();
    model.find_next_match();
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::Center],
    );

    model.find_prev_match();
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::Center],
    );
}

#[test]
fn text_edit_emits_reveal_nearest_edge() {
    let mut model = long_model();
    let _ = model.drain_effects();
    model.insert_text("x".into());
    let intents = only_reveal_intents(&model.drain_effects());
    assert_eq!(intents.last().copied(), Some(RevealIntent::NearestEdge));
}

fn set_cursor_line(model: &mut EditorModel, line: usize) {
    model.move_document_boundary(false, false);
    if line > 0 {
        model.move_logical_rows(line as isize, false);
    }
    let _ = model.drain_effects();
}

fn set_cursor(model: &mut EditorModel, offset: usize, preferred_column: Option<usize>) {
    model.move_to_char(offset, false, preferred_column);
    let _ = model.drain_effects();
}

fn enter_normal(model: &mut EditorModel) {
    model.handle_vim_escape();
    let _ = model.drain_effects();
}

fn ctrl(c: &str) -> (VimKey, VimModifiers) {
    (VimKey::Character(c.into()), VimModifiers::CONTROL)
}

#[test]
fn half_page_down_moves_cursor_by_half_viewport_rows() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 5);

    model.half_page_down(false, 0);
    assert_eq!(model.snapshot().cursor_position.line, 15);
    let intents = only_reveal_intents(&model.drain_effects());
    assert_eq!(intents, vec![RevealIntent::NearestEdge]);
}

#[test]
fn half_page_up_moves_cursor_by_half_viewport_rows() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 50);

    model.half_page_up(false, 0);
    assert_eq!(model.snapshot().cursor_position.line, 40);
}

#[test]
fn half_page_down_at_eof_snaps_to_eol() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 199);

    model.half_page_down(false, 0);
    assert_eq!(model.snapshot().cursor_position.line, 199);
    assert_eq!(
        model.snapshot().cursor_position.column,
        "line 199".chars().count()
    );
}

#[test]
fn half_page_up_at_bof_clamps() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 0);

    model.half_page_up(false, 0);
    assert_eq!(model.snapshot().cursor_position.line, 0);
}

#[test]
fn page_down_respects_current_viewport_rows() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 10);

    model.page_down(false, 0);
    assert_eq!(model.snapshot().cursor_position.line, 28);

    model.set_viewport_rows(10);
    set_cursor_line(&mut model, 10);
    model.page_down(false, 0);
    assert_eq!(model.snapshot().cursor_position.line, 18);
}

#[test]
fn page_down_at_eof_snaps_to_line_end_and_emits_reveal() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 199);
    model.move_horizontal_by(2, false);
    let _ = model.drain_effects();

    model.page_down(false, 0);
    assert_eq!(model.snapshot().cursor_position.line, 199);
    assert_eq!(
        model.snapshot().cursor_position.column,
        "line 199".chars().count()
    );
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::NearestEdge],
    );
}

#[test]
fn page_up_at_bof_snaps_to_line_start_and_emits_reveal() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor(&mut model, 2, None);

    model.page_up(false, 0);
    assert_eq!(model.snapshot().cursor_position.line, 0);
    assert_eq!(model.snapshot().cursor_position.column, 0);
    assert_eq!(
        only_reveal_intents(&model.drain_effects()),
        vec![RevealIntent::NearestEdge],
    );
}

#[test]
fn vim_arrow_down_at_eof_keeps_vim_clamp_behavior() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta",
        )],
        "Ready.".into(),
    );
    set_cursor(&mut model, "alpha\nbe".chars().count(), None);
    enter_normal(&mut model);
    let before = model.snapshot().cursor_position;

    model.handle_vim_key(
        VimKey::Named(VimNamedKey::ArrowDown),
        VimModifiers::default(),
        0,
    );
    assert_eq!(model.snapshot().cursor_position, before);
}

#[test]
fn vim_ctrl_d_moves_cursor_and_emits_nearest_edge() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 30);
    enter_normal(&mut model);

    let (key, mods) = ctrl("d");
    model.handle_vim_key(key, mods, 0);
    assert_eq!(model.snapshot().cursor_position.line, 40);
    let intents = only_reveal_intents(&model.drain_effects());
    assert!(intents.contains(&RevealIntent::NearestEdge));
}

#[test]
fn vim_ctrl_u_moves_cursor_and_emits_nearest_edge() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 80);
    enter_normal(&mut model);

    let (key, mods) = ctrl("u");
    model.handle_vim_key(key, mods, 0);
    assert_eq!(model.snapshot().cursor_position.line, 70);
}

#[test]
fn vim_half_page_commands_keep_clamped_columns_at_document_edges() {
    let mut eof = long_model();
    eof.set_viewport_rows(20);
    set_cursor_line(&mut eof, 199);
    eof.move_horizontal_by(2, false);
    let _ = eof.drain_effects();
    enter_normal(&mut eof);
    let before = eof.snapshot().cursor_position;

    let (key, mods) = ctrl("d");
    eof.handle_vim_key(key, mods, 0);
    assert_eq!(eof.snapshot().cursor_position, before);
    let _ = eof.drain_effects();

    let mut bof = long_model();
    bof.set_viewport_rows(20);
    set_cursor(&mut bof, 2, None);
    enter_normal(&mut bof);
    let before = bof.snapshot().cursor_position;

    let (key, mods) = ctrl("u");
    bof.handle_vim_key(key, mods, 0);
    assert_eq!(bof.snapshot().cursor_position, before);
    let _ = bof.drain_effects();
}

#[test]
fn vim_full_page_commands_keep_clamped_columns_at_document_edges() {
    let mut eof = long_model();
    eof.set_viewport_rows(20);
    set_cursor_line(&mut eof, 199);
    eof.move_horizontal_by(2, false);
    let _ = eof.drain_effects();
    enter_normal(&mut eof);
    let before = eof.snapshot().cursor_position;

    let (key, mods) = ctrl("f");
    eof.handle_vim_key(key, mods, 0);
    assert_eq!(eof.snapshot().cursor_position, before);
    let _ = eof.drain_effects();

    let mut bof = long_model();
    bof.set_viewport_rows(20);
    set_cursor(&mut bof, 2, None);
    enter_normal(&mut bof);
    let before = bof.snapshot().cursor_position;

    let (key, mods) = ctrl("b");
    bof.handle_vim_key(key, mods, 0);
    assert_eq!(bof.snapshot().cursor_position, before);
    let _ = bof.drain_effects();
}

#[test]
fn vim_zz_emits_reveal_center_without_moving_cursor() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 50);
    enter_normal(&mut model);

    model.handle_vim_key(VimKey::Character("z".into()), VimModifiers::default(), 0);
    model.handle_vim_key(VimKey::Character("z".into()), VimModifiers::default(), 0);
    assert_eq!(model.snapshot().cursor_position.line, 50);
    let intents = only_reveal_intents(&model.drain_effects());
    assert!(intents.contains(&RevealIntent::Center));
}

#[test]
fn vim_zt_emits_reveal_top() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 50);
    enter_normal(&mut model);

    model.handle_vim_key(VimKey::Character("z".into()), VimModifiers::default(), 0);
    model.handle_vim_key(VimKey::Character("t".into()), VimModifiers::default(), 0);
    assert_eq!(model.snapshot().cursor_position.line, 50);
    let intents = only_reveal_intents(&model.drain_effects());
    assert!(intents.contains(&RevealIntent::Top));
}

#[test]
fn vim_zb_emits_reveal_bottom() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 50);
    enter_normal(&mut model);

    model.handle_vim_key(VimKey::Character("z".into()), VimModifiers::default(), 0);
    model.handle_vim_key(VimKey::Character("b".into()), VimModifiers::default(), 0);
    assert_eq!(model.snapshot().cursor_position.line, 50);
    let intents = only_reveal_intents(&model.drain_effects());
    assert!(intents.contains(&RevealIntent::Bottom));
}

#[test]
fn vim_capital_h_moves_to_screen_top_plus_scrolloff() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    model.set_viewport_top(100);
    set_cursor_line(&mut model, 150);
    enter_normal(&mut model);

    model.handle_vim_key(VimKey::Character("H".into()), VimModifiers::default(), 0);
    // top=100, scrolloff=4 → cursor at line 104.
    assert_eq!(model.snapshot().cursor_position.line, 104);
}

#[test]
fn vim_capital_l_moves_to_screen_bottom_minus_scrolloff() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    model.set_viewport_top(100);
    set_cursor_line(&mut model, 100);
    enter_normal(&mut model);

    model.handle_vim_key(VimKey::Character("L".into()), VimModifiers::default(), 0);
    // bottom visual row = 100 + 19 = 119, minus scrolloff 4 = 115.
    assert_eq!(model.snapshot().cursor_position.line, 115);
}

#[test]
fn vim_capital_m_moves_to_screen_middle() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    model.set_viewport_top(100);
    set_cursor_line(&mut model, 100);
    enter_normal(&mut model);

    model.handle_vim_key(VimKey::Character("M".into()), VimModifiers::default(), 0);
    // middle = top + (rows-1)/2 = 100 + 9 = 109.
    assert_eq!(model.snapshot().cursor_position.line, 109);
}

fn enter_visual(model: &mut EditorModel) {
    enter_normal(model);
    model.handle_vim_key(VimKey::Character("v".into()), VimModifiers::default(), 0);
    let _ = model.drain_effects();
}

#[test]
fn vim_ctrl_d_in_visual_extends_selection() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 10);
    enter_visual(&mut model);

    let (key, mods) = ctrl("d");
    model.handle_vim_key(key, mods, 0);
    assert_eq!(model.snapshot().cursor_position.line, 20);
    // Selection runs from original anchor (line 10, col 0) to new cursor.
    assert!(model.snapshot().selection.end > model.snapshot().selection.start);
}

#[test]
fn vim_capital_h_in_visual_extends_selection() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    model.set_viewport_top(100);
    set_cursor_line(&mut model, 150);
    enter_visual(&mut model);

    model.handle_vim_key(VimKey::Character("H".into()), VimModifiers::default(), 0);
    assert_eq!(model.snapshot().cursor_position.line, 104);
    assert!(model.snapshot().selection.end > model.snapshot().selection.start);
}

#[test]
fn vim_zz_in_visual_emits_reveal_center() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 50);
    enter_visual(&mut model);

    model.handle_vim_key(VimKey::Character("z".into()), VimModifiers::default(), 0);
    model.handle_vim_key(VimKey::Character("z".into()), VimModifiers::default(), 0);
    assert_eq!(model.snapshot().cursor_position.line, 50);
    let intents = only_reveal_intents(&model.drain_effects());
    assert!(intents.contains(&RevealIntent::Center));
}

#[test]
fn vim_zt_in_visual_emits_reveal_top() {
    let mut model = long_model();
    model.set_viewport_rows(20);
    set_cursor_line(&mut model, 50);
    enter_visual(&mut model);

    model.handle_vim_key(VimKey::Character("z".into()), VimModifiers::default(), 0);
    model.handle_vim_key(VimKey::Character("t".into()), VimModifiers::default(), 0);
    let intents = only_reveal_intents(&model.drain_effects());
    assert!(intents.contains(&RevealIntent::Top));
}

#[test]
fn viewport_scrolloff_shrinks_when_viewport_too_small() {
    use lst_editor::viewport::Viewport;
    let mut v = Viewport {
        rows: 4,
        scrolloff: 4,
        ..Viewport::default()
    };
    // (rows - 1) / 2 = 1; scrolloff min'd to 1.
    assert_eq!(v.effective_scrolloff(), 1);

    v.rows = 1;
    assert_eq!(v.effective_scrolloff(), 0);
}
