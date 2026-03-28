# Ideas

## Session restore

Reopen the tabs you had when you last closed the editor. Store session state
(open file paths, active tab index) in `~/.local/share/lst/session.json`.
On launch with no file arguments, restore the previous session instead of
creating a fresh scratchpad file.

## Recent files

Show a list of recently opened/created files (scratchpad and regular) when
launching without arguments, or via a command palette. Could be a simple
overlay or sidebar listing files from the scratchpad directory sorted by
modification time.
