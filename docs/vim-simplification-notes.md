# Vim Simplification Notes

These are candidate directions for making the Vim engine smaller, clearer, and
faster without weakening the oracle/manual behavior gates.

## High-Leverage Refactors

- Keep the first-party Vim core small enough that command construction stays
  visible in one module. The current core uses direct motion targets plus
  charwise/linewise edit helpers instead of a separate adapter layer.
- Keep line/token helpers local to the operations that need them. A future
  borrowed line view is still worth measuring if it centralizes char length,
  grapheme cells, first non-blank, byte slicing, and ASCII scans without making
  simple motions slower.
- Reuse a token-run iterator for word motions, word text objects, and
  word-under-cursor search where it reduces duplication without broadening the
  command state.
- Keep operator semantics explicit at the call site: linewise vs charwise,
  inclusive vs exclusive, no-op-on-same, and special end-of-line handling.

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

The next serious simplification should be benchmark-driven cleanup inside
`vim_engine.rs`: reduce parser/pending duplication for simple motions, keep word
and text-object logic line-local, and avoid adding an external Vim engine unless
it replaces substantial first-party code while preserving the oracle and X11
contracts.
