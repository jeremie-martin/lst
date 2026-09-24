# lst patches to tree-sitter 0.26.8

This is the crates.io 0.26.8 source (the Rust binding and the C library it
compiles), selected through the workspace patch table. `lst.patch` is the exact
diff against the crates.io sources. Every change is marked `lst patch`.

## Lazy external-scanner state in `changed_ranges`

`ts_tree_get_changed_ranges` walks the old and new trees together. For every
subtree it skips it called `ts_subtree_last_external_token`, which descends to
the subtree's last external token and scans each level's children backwards,
so it could compare scanner states later. The comparison needs that state only
when the two subtrees are otherwise identical and contain external tokens.

The iterator now remembers the last skipped subtree that has external tokens
and finds its last token only when a comparison needs it. When both iterators
hold the same shared subtree, which is the usual case in unchanged regions, the
states are equal without looking. The returned ranges are unchanged.

Measured on the 660 KB Rust benchmark corpus, one-character edits (standalone
probe, 60 edits): `changed_ranges` 0.115 → 0.050 ms back to back and 0.68 →
0.30 ms with a 70 ms pause before each edit (the keystroke case, with caches
cold after idle). A differential fuzzer comparing stock and patched builds on
Rust, Python, Markdown and its inline grammar, YAML, HTML, CSS, JavaScript and
TSX (68 runs of 150–400 random edits, including quotes, comment and raw-string
delimiters, indentation, and newlines) produced byte-identical ranges and trees.

Remove this vendor copy when a tree-sitter release includes an equivalent
change.
