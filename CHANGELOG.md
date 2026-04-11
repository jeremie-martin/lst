# Changelog

## Unreleased

- Removed the legacy iced implementation, benchmark harnesses, and retained auto-benchmark entry points
- Moved GPUI widget code into `apps/lst-gpui/src/ui` instead of keeping a separate app-only crate
- Split editor commands, tabs, and snapshots into focused `lst-editor` modules
- Made the repository root an active GPUI workspace only

## 0.1.0 - 2026-04-10

- First tagged release of `lst`
- Added real-display X11 performance benchmarks for scroll, editing, and large paste workflows
- Added a frozen ~20k-line Rust paste corpus and a completed large-paste benchmark contract
- Documented the local `iced 0.14` text editor performance assessment and the historical paste-path limitations
