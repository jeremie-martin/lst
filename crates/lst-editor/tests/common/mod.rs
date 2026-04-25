use lst_editor::{EditorModel, EditorTab};

pub fn model_with_tabs(tabs: Vec<EditorTab>, status: String) -> EditorModel {
    let mut tabs = tabs.into_iter();
    let first = tabs.next().expect("test model needs at least one tab");
    EditorModel::from_tabs(first, tabs.collect(), status)
}
