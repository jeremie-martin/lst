//! In-process X11 harness for driving the `lst` editor in tests and benchmarks.
//!
//! Spawn the editor via [`Display::spawn_editor`]; drive it through [`Editor`]
//! methods; assert through file/clipboard wait helpers. Effectful actions are
//! fire-and-forget — callers synchronize explicitly via `wait_quiet`,
//! `wait_file_text`, or the helpers in [`clipboard`].
//!
//! Requires a real X server (set `DISPLAY`) and `xclip` on `PATH`.

mod display;
mod editor;
mod x11;

pub mod clipboard;
pub mod state_trace;

pub use clipboard::Selection;
pub use display::Display;
pub use editor::{ChordMods, Editor, FileStats, FileWaitOpts, FileWaitOutcome, Key, KeyChord, SpawnOpts, WheelDir};
pub use state_trace::{
    StateTraceReader, StateTraceRecord, TraceCursor, TraceFind, TraceRange, TraceRow, TraceViewport,
    STATE_TRACE_SCHEMA_VERSION,
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync + 'static>>;
