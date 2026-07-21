# UI Quality Contract

The editor should feel quiet, precise, and intentional. Visual correctness is
part of product behavior: a repeatable glitch is still a glitch, and a pixel
baseline can preserve a bad decision just as easily as a good one.

## Layout and ownership

- Derive related geometry once and pass the result to every consumer. Paint,
  hit testing, wrapping, scrolling, cursor reveal, IME bounds, and state traces
  must not independently reconstruct the same coordinate.
- Give every border edge one owner. A surface owns its outer edge; one sibling
  owns an internal separator. Never compose adjacent borders to simulate one
  line.
- Keep the main shell continuous. Add space or a stronger separator only when
  it communicates grouping, state, or interaction.
- Prefer stable dimensions, but let content capacity drive them at meaningful
  thresholds. The gutter reserves three digits and changes only at powers of
  ten.
- Treat unexplained arithmetic (`+ 1`, `- 8`) as a review prompt. Name the
  spacing or encode it in a layout value when it represents a real concept.

## Typography and color

- Chrome uses the stable UI font; document text and gutter digits use the
  configurable editor font. Changing the code font must not unexpectedly
  reflow menus or status chrome.
- Choose sizes from the shared UI type scale. Different sizes are welcome when
  they express hierarchy; adjacent peers should not drift by one pixel without
  a reason.
- Use semantic theme roles. Selection, passive occurrence, search, active
  search, caret, and current-line states must remain distinguishable in both
  themes.
- Prefer foreground emphasis for active gutter numbers. The gutter background
  is the editor background and has no divider.
- Check small muted text against its actual surface in both themes. Do not rely
  on a color name such as `muted` as evidence that contrast is sufficient.

## Rendering and latency

- Work in proportion to what changed and what is visible. Passive occurrence
  highlighting starts from cached painted character windows, merges adjacent
  wrapped windows, and expands only to the surrounding identifier boundaries.
  Do not traverse one long identifier once per painted row or allocate a copy
  of the whole document on every cursor movement.
- Define decoration precedence explicitly: current line, passive occurrences,
  search matches, active search match, selections, text, then carets.
- Cache keys must contain every input that changes visible output, including
  theme, active state, revision, query, and visible range where applicable.
- Exercise `typing-large`, scrolling, and search benchmarks after changing a
  cursor-driven or paint-driven path. Include minified or otherwise very long
  logical lines when work is meant to be viewport-bounded. Rare layout
  transitions, such as a new gutter digit, should still preserve cursor
  visibility and viewport validity.

## Review and testing

- Specify observable behavior through X11: geometry thresholds, visible
  decoration ranges, pointer mapping, focus, and responsive state.
- Maintain exact-pixel baselines for representative dark and light surfaces.
  Review the image itself before accepting a baseline update.
- Include zoom, narrow-window, long-label, empty-state, multi-cursor, wrapped,
  and scrolled cases when they can stress the changed contract.
- Prefer construction that prevents misalignment over a regression test that
  merely detects it. Tests guard the contract; ownership should make the
  invalid composition difficult to express.
