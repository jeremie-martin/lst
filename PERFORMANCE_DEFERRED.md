This document captures optimization and refactor opportunities identified during the wrapped-layout and deferred-find performance pass that were intentionally deferred to keep the current change focused on typing and scrolling CPU cost.

## Delta-Based Undo History

Current issue: undo stores whole-buffer snapshots, so large files pay repeated full-text copies in both memory and CPU.

Proposed optimization/refactor: replace snapshot storage with deltas or a piece-table/rope-backed history that records edits instead of full document states.

Why deferred: changing undo storage would touch most mutation paths, saved-state tracking, and redo semantics, which is a larger correctness surface than this pass.

Expected benefit: lower peak memory use, less allocation churn on edit-heavy sessions, and cheaper undo snapshot creation.

Implementation risk/complexity: high. The editor currently assumes snapshot restore is cheap and uniform across normal edits, Vim commands, and line transforms.

Revisit condition: prioritize this once profiling shows memory growth or snapshot copying is still a top cost after the layout and find changes land.

## Incremental Line Editing for Vim and Helpers

Current issue: many Vim commands and line helpers rebuild the whole document string and replace `text_editor::Content` even when they only affect one or two lines.

Proposed optimization/refactor: move line-local commands toward incremental edits or a small internal line buffer abstraction that can patch only the touched range before syncing back to the editor widget.

Why deferred: this editor relies on a mix of direct `text_editor::Action` calls and whole-buffer rebuilds today, so incrementalizing the command layer cleanly needs a broader edit model decision.

Expected benefit: lower latency for repeated line operations, less allocation churn, and fewer full-buffer invalidations of related caches.

Implementation risk/complexity: medium to high. The command set is broad and includes modal/Vim behaviors that depend on exact cursor and selection semantics.

Revisit condition: prioritize this if line-wise commands, Vim operators, or replace-style helpers still show noticeable hitches on medium and large files.

## Incremental Find Index Maintenance

Current issue: the current pass only defers live find refresh after edits; it still recomputes matches from the whole document on explicit refresh points.

Proposed optimization/refactor: maintain match positions incrementally for line-local edits, or introduce a lightweight per-line index so only changed lines need to be rescanned.

Why deferred: deferred refresh removes the hot-path typing cost without adding complex invalidation logic, which was the primary goal of this pass.

Expected benefit: faster find refresh on large files, more accurate live match counts with lower CPU cost, and fewer whole-buffer string walks during search-heavy workflows.

Implementation risk/complexity: medium. Incremental match maintenance needs careful handling for multiline cursor movement, wraparound navigation, and replacement operations.

Revisit condition: prioritize this if explicit find navigation or idle refresh remains measurably expensive after the deferred-refresh model ships.

## Incremental Wrapped Layout Cache Updates

Current issue: the current pass removed whole-document wrap/gutter work from redraw and scroll paths, but it still rebuilds the wrapped layout cache after document mutations, so very large wrapped files can still pay O(file size) layout work while typing.

Proposed optimization/refactor: update cached wrapped row counts and prefix offsets incrementally for the affected line range, instead of rebuilding the whole cache after every content revision.

Why deferred: doing this correctly needs a clearer edit-delta model across direct text editor actions, rebuild-style transforms, and Vim helpers, which is larger than the intended scope of this pass.

Expected benefit: lower typing latency in large wrapped files while preserving the redraw and scroll gains from the current cache structure.

Implementation risk/complexity: medium to high. The cache has to stay correct across inserts, deletes, line splits/joins, and multi-line rebuild operations.

Revisit condition: prioritize this if typing in large wrapped files is still a measurable hotspot after the current redraw, scroll, and deferred-find changes ship.

## Syntax Highlighting Profiling and Caching

Current issue: syntax highlighting still performs per-line parsing/highlighting work, especially for `syntect`-backed files, and it was not changed in this pass.

Proposed optimization/refactor: profile the current renderer path first, then add targeted caching or batching only if highlighting remains a top contributor after layout caching.

Why deferred: layout, gutter, and find rescans were the clearer structural hot paths and needed to be removed before judging the remaining highlight cost accurately.

Expected benefit: lower redraw cost on syntax-heavy files and less CPU spent re-highlighting unchanged regions.

Implementation risk/complexity: medium. Highlight caches must stay correct across file type changes, Markdown fence transitions, and partial invalidation.

Revisit condition: prioritize this only if post-change profiling still points to highlight work as a dominant redraw hotspot.

## Remove the Multi-Click Drag Workaround

Current issue: the editor wraps the text editor in a `mouse_area` and manually forwards drag updates to work around upstream `iced` multi-click drag behavior.

Proposed optimization/refactor: remove the workaround and simplify the event path once the upstream `iced` issue is fixed in the version this project targets.

Why deferred: the workaround is still functionally required today, and removing it before the upstream fix would regress selection behavior.

Expected benefit: less custom event plumbing, fewer mouse-move code paths to maintain, and a cleaner editor input stack.

Implementation risk/complexity: low once upstream behavior is verified; medium before that because selection semantics are user-visible.

Revisit condition: prioritize this when upgrading `iced` to a version that closes the relevant upstream issue and preserves current selection behavior in manual testing.
