use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::{Context, Window};

use crate::{
    recent::{
        normalize_recent_path, read_recent_preview, search_recent_content, ApplyPreviewOutcome,
        RecentPreviewRead, RecentSelectionMove,
    },
    ui::{InputFieldEvent, InputFieldNavigation},
    viewport::{reset_scroll, scroll_to_top},
    LstGpuiApp,
};

#[cfg(not(test))]
const RECENT_CONTENT_SEARCH_DEBOUNCE_MS: u64 = 200;

impl LstGpuiApp {
    pub(crate) fn toggle_recent_files_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.recent.is_open() {
            self.close_recent_files_panel(cx);
            return;
        }

        let pending_search = self.recent.open();
        let seeded_query = self.recent.query().to_string();
        reset_scroll(&self.recent_scroll);
        self.recent_query_input
            .update(cx, |input, cx| input.set_text(&seeded_query, cx));
        let focus_handle = self.recent_query_input.read(cx).focus_handle();
        window.focus(&focus_handle);
        if let Some(generation) = pending_search {
            self.schedule_recent_content_search(generation, cx);
        }
        self.spawn_recent_previews(cx);
        cx.notify();
    }

    pub(crate) fn close_recent_files_panel(&mut self, cx: &mut Context<Self>) {
        if self.recent.is_open() {
            self.recent.close();
            self.force_editor_focus = true;
            cx.notify();
        }
    }

    pub(crate) fn handle_recent_query_input_event(
        &mut self,
        event: &InputFieldEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputFieldEvent::Changed(text) => {
                self.update_recent_query(text.clone(), cx);
                cx.notify();
            }
            InputFieldEvent::Submitted => {
                if let Some(path) = self.recent.selected_path() {
                    self.open_recent_path(path, cx);
                }
            }
            InputFieldEvent::Cancelled => self.close_recent_files_panel(cx),
            InputFieldEvent::NextRequested => {
                self.move_recent_selection(RecentSelectionMove::Next, cx);
            }
            InputFieldEvent::PreviousRequested => {
                self.move_recent_selection(RecentSelectionMove::Previous, cx);
            }
            InputFieldEvent::Navigate(navigation) => match navigation {
                InputFieldNavigation::Up => {
                    self.move_recent_selection(RecentSelectionMove::RowPrevious, cx);
                }
                InputFieldNavigation::Down => {
                    self.move_recent_selection(RecentSelectionMove::RowNext, cx);
                }
            },
        }
    }

    fn update_recent_query(&mut self, text: String, cx: &mut Context<Self>) {
        let pending_search = self.recent.set_query(text);
        reset_scroll(&self.recent_scroll);
        self.spawn_recent_previews(cx);
        if let Some(generation) = pending_search {
            self.schedule_recent_content_search(generation, cx);
        }
    }

    fn move_recent_selection(&mut self, movement: RecentSelectionMove, cx: &mut Context<Self>) {
        if let Some(index) = self.recent.move_selection(movement) {
            self.scroll_recent_selection_into_view(index);
        }
        cx.notify();
    }

    fn scroll_recent_selection_into_view(&self, index: usize) {
        let Some(bounds) = self.recent.card_bounds_for(index) else {
            return;
        };

        let viewport = self.recent_scroll.bounds();
        if viewport.size.height <= gpui::px(0.0) {
            return;
        }

        let offset_y = self.recent_scroll.offset().y;
        let target = if bounds.top() + offset_y < viewport.top() {
            bounds.top() - viewport.top()
        } else if bounds.bottom() + offset_y > viewport.bottom() {
            bounds.bottom() - viewport.bottom()
        } else {
            return;
        };
        scroll_to_top(&self.recent_scroll, target);
    }

    pub(crate) fn load_more_recent_files(&mut self, cx: &mut Context<Self>) {
        self.recent.load_more();
        self.spawn_recent_previews(cx);
        cx.notify();
    }

    fn spawn_recent_previews(&mut self, cx: &mut Context<Self>) {
        for path in self.recent.paths_to_load_previews() {
            cx.spawn(async move |this, cx| {
                let preview_path = path.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { read_recent_preview(&preview_path) })
                    .await;
                let _ = this.update(cx, |view, cx| view.finish_recent_preview(path, result, cx));
            })
            .detach();
        }
    }

    fn schedule_recent_content_search(&mut self, generation: u64, cx: &mut Context<Self>) {
        let query = self.recent.query().trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(recent_content_search_debounce())
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.recent.search_still_relevant(generation, &query) {
                    view.start_recent_content_search(query, cx);
                }
            });
        })
        .detach();
    }

    fn start_recent_content_search(&mut self, query: String, cx: &mut Context<Self>) {
        if !self.recent.start_content_search(query.clone()) {
            return;
        }

        let paths = self.recent.entries().to_vec();
        cx.spawn(async move |this, cx| {
            let search_query = query.clone();
            let matches = cx
                .background_executor()
                .spawn(async move { search_recent_content(paths, &search_query) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.finish_recent_content_search(query, matches, cx);
            });
        })
        .detach();
    }

    fn finish_recent_content_search(
        &mut self,
        query: String,
        matches: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.recent.finish_content_search(query, matches) {
            self.spawn_recent_previews(cx);
            cx.notify();
        }
    }

    fn finish_recent_preview(
        &mut self,
        path: PathBuf,
        result: RecentPreviewRead,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            self.recent.apply_preview(path, result),
            ApplyPreviewOutcome::Pruned
        ) {
            self.spawn_recent_previews(cx);
        }
        cx.notify();
    }

    pub(crate) fn open_recent_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = normalize_recent_path(&path);
        if self.activate_existing_tab_for_path(&path, cx) {
            self.recent.record(&path);
            self.close_recent_files_panel(cx);
            return;
        }

        match crate::runtime::read_file_with_stamp(&path) {
            Ok((text, stamp)) => {
                let opened_path = path.clone();
                self.update_model(cx, true, |model| {
                    model.open_files_with_stamps(vec![(opened_path, text, Some(stamp))]);
                });
                self.recent.record(&path);
                self.close_recent_files_panel(cx);
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    self.recent.prune_path(&path);
                    self.spawn_recent_previews(cx);
                }
                self.update_model(cx, true, |model| {
                    model.open_file_failed(path, err.to_string());
                });
            }
        }
    }

    fn activate_existing_tab_for_path(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let Some(tab_id) = self.model.tabs().iter().find_map(|tab| {
            let tab_path = tab.path()?;
            (normalize_recent_path(tab_path) == path).then_some(tab.id())
        }) else {
            return false;
        };

        self.update_model(cx, true, |model| {
            model.set_active_tab(tab_id);
        });
        true
    }
}

fn recent_content_search_debounce() -> Duration {
    #[cfg(test)]
    {
        Duration::from_millis(0)
    }
    #[cfg(not(test))]
    {
        Duration::from_millis(RECENT_CONTENT_SEARCH_DEBOUNCE_MS)
    }
}
