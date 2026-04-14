//! Logical viewport state for the editor model.
//!
//! The editor crate is normally framework-neutral and scroll-agnostic, but a
//! handful of commands (half-page motion, `H`/`M`/`L`, centered reveals) only
//! make sense when the model knows how many visual rows are currently on
//! screen and which row sits at the top. The UI layer pushes that information
//! into the model on layout and scroll changes; the model uses it to compute
//! cursor targets and reveal intents.
//!
//! Pixel-level scroll still belongs to the UI layer — this struct only speaks
//! in visual rows.

pub const DEFAULT_VIEWPORT_ROWS: usize = 24;
pub const DEFAULT_SCROLLOFF: usize = 4;
pub const DEFAULT_SIDESCROLLOFF: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    /// Number of visible visual rows, as reported by the UI layer.
    pub rows: usize,
    /// First visible visual row. Echoed from the UI layer on scroll.
    pub top_visual_row: usize,
    /// Minimum rows kept between the cursor and the viewport's vertical edges.
    pub scrolloff: usize,
    /// Minimum columns kept between the cursor and horizontal edges when
    /// soft-wrap is disabled. Unused while soft-wrap is on.
    pub sidescrolloff: usize,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            rows: DEFAULT_VIEWPORT_ROWS,
            top_visual_row: 0,
            scrolloff: DEFAULT_SCROLLOFF,
            sidescrolloff: DEFAULT_SIDESCROLLOFF,
        }
    }
}

impl Viewport {
    /// Rows used for Ctrl-D / Ctrl-U. Always at least 1.
    pub fn half_page(&self) -> usize {
        (self.rows / 2).max(1)
    }

    /// Rows used for Ctrl-F / Ctrl-B / PageDown / PageUp. Leaves a small
    /// overlap for orientation.
    pub fn page(&self) -> usize {
        self.rows.saturating_sub(2).max(1)
    }

    /// Effective scrolloff — shrinks gracefully when the viewport is too
    /// small to accommodate `scrolloff` on both sides.
    pub fn effective_scrolloff(&self) -> usize {
        if self.rows <= 1 {
            return 0;
        }
        self.scrolloff.min((self.rows - 1) / 2)
    }

    /// The visual row a cursor should land on for `H` (screen top).
    pub fn screen_top_row(&self) -> usize {
        self.top_visual_row
            .saturating_add(self.effective_scrolloff())
    }

    /// The visual row a cursor should land on for `M` (screen middle).
    pub fn screen_middle_row(&self) -> usize {
        self.top_visual_row + self.rows.saturating_sub(1) / 2
    }

    /// The visual row a cursor should land on for `L` (screen bottom).
    pub fn screen_bottom_row(&self) -> usize {
        let scrolloff = self.effective_scrolloff();
        let bottom = self.top_visual_row + self.rows.saturating_sub(1);
        bottom.saturating_sub(scrolloff)
    }
}
