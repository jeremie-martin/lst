# Changelog

## Unreleased

- Added tree-sitter syntax highlighting for Rust, Python, JavaScript/JSX, TypeScript/TSX, JSON, TOML, YAML, Markdown, HTML, and CSS, with incremental reparsing and language injection
- Added AI text cleanup (`Ctrl-Shift-R` / status-bar sparkle button) that rewrites the buffer or selection through DeepSeek to remove transcription artifacts
- Added per-buffer line bookmarks with toggle and next/previous navigation (`Ctrl-Alt-K` / `Ctrl-Alt-L` / `Ctrl-Alt-J`)
- Removed stale benchmark corpus snapshots and moved active benchmarks to generated deterministic corpora
- Removed the `lst-gpui` install compatibility alias
- Removed backward-compatible editor construction adapters that allowed empty tab sets
- Removed the legacy iced implementation and old benchmark harnesses
- Moved GPUI widget code into `apps/lst-gpui/src/ui` instead of keeping a separate app-only crate
- Split editor commands, tabs, and snapshots into focused `lst-editor` modules
- Made the repository root an active GPUI workspace only

## 0.1.0 - 2026-04-10

- First tagged release of `lst`
- Added real-display X11 performance benchmarks for scroll, editing, and large paste workflows
- Added a frozen ~20k-line Rust paste corpus and a completed large-paste benchmark contract
- Documented the local `iced 0.14` text editor performance assessment and the historical paste-path limitations
