use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

pub type DialogFuture = Pin<Box<dyn Future<Output = Option<PathBuf>> + Send + 'static>>;

pub trait Dialogs: Send + Sync {
    fn pick_open_file(&self) -> DialogFuture;
    fn pick_save_file(&self, suggested_name: &str) -> DialogFuture;
    fn pick_open_file_blocking(&self) -> Option<PathBuf>;
    fn pick_save_file_blocking(&self, suggested_name: &str) -> Option<PathBuf>;
}

pub type SharedDialogs = Arc<dyn Dialogs>;

pub struct RealDialogs;

impl Dialogs for RealDialogs {
    fn pick_open_file(&self) -> DialogFuture {
        Box::pin(async move {
            rfd::AsyncFileDialog::new()
                .add_filter(
                    "Text",
                    &["txt", "md", "rs", "py", "toml", "yaml", "json", "sh"],
                )
                .add_filter("All files", &["*"])
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        })
    }

    fn pick_save_file(&self, suggested_name: &str) -> DialogFuture {
        let suggested_name = suggested_name.to_string();
        Box::pin(async move {
            rfd::AsyncFileDialog::new()
                .set_file_name(&suggested_name)
                .save_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        })
    }

    fn pick_open_file_blocking(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter(
                "Text",
                &["txt", "md", "rs", "py", "toml", "yaml", "json", "sh"],
            )
            .add_filter("All files", &["*"])
            .pick_file()
    }

    fn pick_save_file_blocking(&self, suggested_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(suggested_name)
            .save_file()
    }
}

pub struct NullDialogs;

impl Dialogs for NullDialogs {
    fn pick_open_file(&self) -> DialogFuture {
        Box::pin(async { None })
    }

    fn pick_save_file(&self, _suggested_name: &str) -> DialogFuture {
        Box::pin(async { None })
    }

    fn pick_open_file_blocking(&self) -> Option<PathBuf> {
        None
    }

    fn pick_save_file_blocking(&self, _suggested_name: &str) -> Option<PathBuf> {
        None
    }
}
