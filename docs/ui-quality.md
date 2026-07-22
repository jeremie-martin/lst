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
- Use semantic theme roles. Active and inactive selections/current lines,
  selected-text matches, passive occurrences, search states, and primary and
  secondary carets must form a distinguishable hierarchy in both themes.
- Prefer foreground emphasis for active gutter numbers. The gutter background
  is the editor background and has no divider.
- Check small muted text against its actual surface in both themes. Do not rely
  on a color name such as `muted` as evidence that contrast is sufficient.
- Audit normal small text and bracket glyphs at 4.5:1 or better against their
  real surface. Focus outlines, control boundaries, guides, and other
  non-text indicators target 3:1. Disabled controls are the deliberate
  exception, not a source for active-state colors.
- Interactive controls use one state order: disabled, pressed, selected,
  hovered, with a separate focus outline. A selected value and keyboard focus
  are different states and must remain independently visible.

## Rendering and latency

- Work in proportion to what changed and what is visible. Passive occurrence
  highlighting starts from cached painted character windows, merges adjacent
  wrapped windows, and expands only to the surrounding identifier boundaries.
  Do not traverse one long identifier once per painted row or allocate a copy
  of the whole document on every cursor movement.
- Drive passive decorations from interaction state, not incidental paint state.
  Typing invalidates occurrence highlights; editor focus or an explicit caret
  move establishes the next query. Both caret edges belong to the word they
  touch, while separator interiors do not.
- Treat an explicit selection as a different query from a passive caret word.
  Selected-text matches are exact and case-sensitive, exclude every selected
  range, and are disabled for multiline, whitespace-only, over-200-character,
  or mutually different multi-selections. Scan only query-length-expanded
  painted windows, and keep match endpoints on grapheme boundaries. When a
  literal, non-whole-word find query already represents the same text, let the
  find decorations own those pixels and skip the duplicate selection scan.
- Define decoration precedence explicitly: rulers and guides, current line,
  passive occurrences and selected-text matches, bracket backgrounds, search
  matches, selections, text and bracket colors, whitespace/control markers,
  bracket outlines, then carets.
- Structural decoration lookup must be logarithmic plus visible output. Use
  sorted bracket tokens and the pair-parent chain; never scan every pair for
  every cursor or every historical scope for a viewport near end-of-file.
- Cursor preparation slices the sorted cursor set once per painted row. Do not
  re-scan every cursor to answer each row's current-line and caret questions.
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
  Review the image itself before accepting a baseline update; repeatability
  alone is insufficient if a physical compositor captured another surface.
- Include zoom, narrow-window, long-label, empty-state, multi-cursor, wrapped,
  and scrolled cases when they can stress the changed contract.
- Exercise settings and dialogs at 640×480/100% and 900×600/maximum zoom, in
  both themes, with wrapping and panels open. Controls may wrap; labels must
  not overlap, clip critical actions, or create two owners for one border.
- Prefer construction that prevents misalignment over a regression test that
  merely detects it. Tests guard the contract; ownership should make the
  invalid composition difficult to express.
