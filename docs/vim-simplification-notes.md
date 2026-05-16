# Vim Simplification Notes

These are candidate directions for making the Vim engine smaller, clearer, and
faster without weakening the oracle/manual behavior gates.

## High-Leverage Refactors

- Introduce a shared target primitive for cursor motions, charwise ranges, and
  linewise ranges. Operators and visual selection should consume this instead
  of each path rebuilding its own `(Position, Position)` or line span.
- Keep `VimText` borrowed, but make line access more structured. A future
  borrowed line view could centralize char length, grapheme cells, first
  non-blank, byte slicing, and ASCII scans for quotes/brackets.
- Collapse command construction around a small set of command shapes: move,
  select, operate, paste-over-selection, shift, transform case, and enter
  insert. This should remove repeated `Vec<VimCommand>` assembly.
- Reuse a token-run iterator for word motions, word text objects, and
  word-under-cursor search. The current code still has several independent
  whitespace/token-boundary loops.
- Make operator semantics explicit as data: linewise vs charwise, inclusive vs
  exclusive, no-op-on-same, and special end-of-line handling.

## Performance Ideas

- Cache grapheme cells per touched line during one Vim key handling call rather
  than globally. That avoids recomputing cells for one command while keeping
  document mutation cheap.
- Add ASCII fast paths for common word, quote, bracket, and search operations;
  preserve the Unicode-aware path as the fallback.
- Keep edit-after-mutation benchmarks honest. Any change in motion/range code
  should be checked against the large mutation benchmark plus word/search/copy
  benchmarks.
- Benchmark Escape after huge edits separately. Escape intentionally warms the
  line cache, while ordinary normal-mode Vim keys should avoid rebuilding it.

## Correctness Gaps To Keep Exercising

- Operator ranges around CRLF, combining clusters, empty last lines, and final
  unterminated lines.
- Counts on text objects and linewise operators.
- Visual reverse selections, VisualLine mode toggles, and visual text objects.
- Operator plus search/find motions, especially when the motion fails or wraps.

## Near-Term Direction

The next serious simplification should introduce a `Target`/`RangeTarget`
primitive, route operators and visual selection through it, and then delete the
old ad hoc range conversion helpers. The point is not just fewer lines: the
engine should have one place that decides how a motion becomes a user-visible
span.
