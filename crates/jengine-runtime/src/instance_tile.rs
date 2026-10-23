//! Several copies of a game side by side: the editor's *Build & Run* starts
//! one runtime per instance (a multiplayer game wants two or four) and tells
//! each which it is with `JENGINE_TILE="<n>/<count>"`, 1-based. The window
//! then takes its cell of a grid over the primary monitor - two side by
//! side, three or four in quarters - and says which it is in its title, so
//! four windows of one game are not four windows stacked on each other.
//!
//! Each instance is also given its own `JENGINE_SAVE_DIR` by the launcher,
//! so its saves (a signed-in player's token among them) are its own.

/// Which instance this is, of how many.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    pub index: u32,
    pub count: u32,
}

impl Tile {
    /// `"2/4"` - instance 2 of 4. Anything else, or one of one, is no tile.
    pub fn parse(text: &str) -> Option<Tile> {
        let (i, n) = text.trim().split_once('/')?;
        let (index, count) = (i.trim().parse::<u32>().ok()?, n.trim().parse::<u32>().ok()?);
        (count > 1 && (1..=count).contains(&index)).then_some(Tile { index, count })
    }

    pub fn from_env() -> Option<Tile> {
        std::env::var("JENGINE_TILE").ok().as_deref().and_then(Tile::parse)
    }

    /// Columns and rows: two side by side, then a square-ish grid.
    pub fn grid(self) -> (u32, u32) {
        let cols = (self.count as f64).sqrt().ceil() as u32;
        let cols = cols.max(2);
        let rows = self.count.div_ceil(cols);
        (cols, rows)
    }

    /// This instance's cell of `area` (x, y, width, height in physical
    /// pixels), filled left to right, top to bottom.
    pub fn cell(self, area: (i32, i32, u32, u32)) -> (i32, i32, u32, u32) {
        let (cols, rows) = self.grid();
        let (x, y, w, h) = area;
        let (cw, ch) = (w / cols, h / rows);
        let k = self.index - 1;
        (x + (k % cols * cw) as i32, y + (k / cols * ch) as i32, cw, ch)
    }

    pub fn title_suffix(self) -> String {
        format!(" - instance {}/{}", self.index, self.count)
    }
}

/// The screen a window can use: the monitor, less the taskbar - which winit
/// cannot see, so a strip at the bottom is kept for it.
pub fn usable_area(monitor_pos: (i32, i32), monitor_size: (u32, u32)) -> (i32, i32, u32, u32) {
    const TASKBAR: u32 = 48;
    (monitor_pos.0, monitor_pos.1, monitor_size.0, monitor_size.1.saturating_sub(TASKBAR))
}

/// A window's frame around its inner area: the title bar and borders. The
/// cell is the outer size, so the inner one is this much smaller.
pub const FRAME: (u32, u32) = (16, 39);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_is_read_from_its_text() {
        assert_eq!(Tile::parse("2/4"), Some(Tile { index: 2, count: 4 }));
        assert_eq!(Tile::parse(" 1 / 2 "), Some(Tile { index: 1, count: 2 }));
        // One of one is a normal window; out of range or nonsense is too.
        for bad in ["1/1", "0/2", "5/4", "two", "", "3"] {
            assert_eq!(Tile::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn instances_share_the_screen_without_overlapping() {
        let area = (0, 0, 1920, 1032);
        // Two: halves, side by side.
        let two = |i| Tile { index: i, count: 2 }.cell(area);
        assert_eq!((two(1), two(2)), ((0, 0, 960, 1032), (960, 0, 960, 1032)));
        // Three and four: quarters, the fourth empty for three.
        for count in [3, 4] {
            let cells: Vec<_> = (1..=count).map(|i| Tile { index: i, count }.cell(area)).collect();
            assert_eq!(cells[0], (0, 0, 960, 516));
            assert_eq!(cells[2], (0, 516, 960, 516));
            for (a, b) in cells.iter().zip(cells.iter().skip(1)) {
                assert_ne!(a, b);
            }
        }
        // On a second monitor to the left, cells start where it does.
        assert_eq!(Tile { index: 2, count: 2 }.cell((-1920, 0, 1920, 1032)).0, -960);
    }
}
