# Changelog

## Unreleased

- Review polished prompts in the editor before applying, with compact adaptive word diffs, a clean Result view, and Apply/Discard controls

- Add a bottom-bar Polish Prompt button

- Replace AI text cleanup with Polish Agent Prompt, using the installed `prompt-add` filter and its editorial behavior and history

- Cut key-to-paint latency roughly in half on X11 (navigation p50 10.6 -> 5.3 ms, typing 11.2 -> 6.8 ms) by drawing right after input instead of at the next refresh tick, and animate scrolling at the monitor's refresh rate
- Cut startup by ~70 ms (parallel font scan, Vulkan context on a startup thread, non-blocking window title) and per-frame viewport work by ~60% (pass-based painting, gutter digit cells, cached glyph tiles, cheaper marker and highlight scans)
- Fixed the benchmark runner: it measured a `test-support` build of the app and phase-locked its key injection to GPUI's refresh timer; it now measures the production build with damage reports paired to the app's frame stamps
- Build against a vendored gpui 0.2.2 (`vendor/gpui`, patches listed in `vendor/gpui/LST_PATCHES.md`)
- Added tree-sitter syntax highlighting for Rust, Python, JavaScript/JSX, TypeScript/TSX, JSON, TOML, YAML, Markdown, HTML, and CSS, with incremental reparsing and language injection
- Added command-palette and status-bar AI text cleanup that rewrites the buffer or selection through DeepSeek to remove transcription artifacts
- Added per-buffer line bookmarks with toggle and next/previous navigation (`Ctrl-Alt-K` / `Ctrl-Alt-L` / `Ctrl-Alt-J`)
- Removed stale benchmark corpus snapshots and moved active benchmarks to generated deterministic corpora
- Removed the `lst-gpui` install compatibility alias
- Removed backward-compatible editor construction adapters that allowed empty tab sets
- Removed the legacy iced implementation and old benchmark harnesses
- Moved GPUI widget code into `apps/lst-gpui/src/ui` instead of keeping a separate app-only crate
- Split editor commands, tabs, and snapshots into focused `lst-editor` modules
- Made the repository root a workspace for the GPUI app and framework-neutral editor

## 0.1.0 - 2026-04-10

- First tagged release of `lst`
- Added real-display X11 performance benchmarks for scroll, editing, and large paste workflows
- Added a frozen ~20k-line Rust paste corpus and a completed large-paste benchmark contract
- Documented the local `iced 0.14` text editor performance assessment and the historical paste-path limitations
