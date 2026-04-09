mod common;

use common::*;
use iced::keyboard;
use iced::widget::text_editor;
use iced::Point;
use lst::app::Message;

#[test]
fn gutter_click_selects_a_line_and_syncs_primary_selection() {
    let mut app = AppHarness::new("aaa\nbbb\nccc");

    app.gutter_click_line(1);

    let snap = app.snapshot();
    let selection = snap.selection.expect("expected selected line");
    let primary = app.clipboard.get_primary();

    assert!(!selection.is_empty(), "selection was {selection:?}");
    assert_eq!(primary, selection, "primary was {primary:?}");
}

#[test]
fn shift_click_extends_selection() {
    let mut app = AppHarness::new("hello world");
    move_to_end(&mut app.app);
    app.send(Message::ModifiersChanged(keyboard::Modifiers::SHIFT));

    app.send(Message::Edit(text_editor::Action::Click(Point::new(
        0.0, 0.0,
    ))));

    assert!(app.snapshot().selection.is_some());
}
