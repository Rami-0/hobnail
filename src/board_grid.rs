//! The board canvas grid: 12 columns of integer cells, a fixed row height
//! chosen by the client. Items are rectangles (`x`, `y`, `w`, `h`) inside
//! one section. The client compacts vertically (gridstack-style gravity);
//! the server checks bounds and overlap and applies the same compaction, so
//! a stored layout never has gaps the client would not have produced.

/// Columns of the canvas.
pub const COLUMNS: u16 = 12;
/// Tallest item, in rows.
pub const MAX_H: u16 = 16;
/// Items a board may hold (validated in the API).
pub const MAX_ITEMS: u16 = 512;
/// Rows a section may extend to: every item of a full board stacked at
/// its tallest. A compacted layout never needs more, so anything beyond is
/// refused before any work proportional to `y` could happen.
pub const MAX_ROWS: u16 = MAX_ITEMS * MAX_H;

/// All sums are taken in `u32`: `u16` fields straight from a request may be
/// near `u16::MAX`, and an overflow would panic (debug) or wrap past a
/// bounds check (release).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn right(&self) -> u32 {
        u32::from(self.x) + u32::from(self.w)
    }

    pub fn bottom(&self) -> u32 {
        u32::from(self.y) + u32::from(self.h)
    }

    /// Whether the two share any column.
    pub fn spans_with(&self, other: &Rect) -> bool {
        u32::from(self.x) < other.right() && u32::from(other.x) < self.right()
    }

    pub fn collides(&self, other: &Rect) -> bool {
        self.spans_with(other)
            && u32::from(self.y) < other.bottom()
            && u32::from(other.y) < self.bottom()
    }

    pub fn in_bounds(&self) -> bool {
        self.x < COLUMNS
            && (1..=COLUMNS).contains(&self.w)
            && (1..=MAX_H).contains(&self.h)
            && self.right() <= u32::from(COLUMNS)
            && self.bottom() <= u32::from(MAX_ROWS)
    }
}

/// The size a new item of `kind` gets (columns, rows).
pub fn default_size(kind: &str) -> (u16, u16) {
    match kind {
        "link" => (3, 1),
        "widget" => (4, 3),
        _ => (4, 2),
    }
}

/// The size of an item saved before the canvas: the old desktop span picked
/// one, two or three of the three auto-fill columns, and `rows` the height.
pub fn legacy_size(kind: &str, desktop: u16, rows: u16) -> (u16, u16) {
    let w = match desktop {
        0..=4 => 4,
        5..=8 => 8,
        _ => 12,
    };
    let (_, h) = default_size(kind);
    let extra = rows.clamp(1, 4) - 1;
    (w, (h + extra * 2).min(MAX_H))
}

/// The top-most, then left-most, free slot of `w` × `h` among `placed`.
/// Such a slot starts at 0 or where a placed item ends, in both axes.
/// Past `MAX_ROWS` (more items than a board may hold) it returns the last
/// row, which validation then refuses as overlapping.
pub fn first_free(placed: &[Rect], w: u16, h: u16) -> Rect {
    let w = w.clamp(1, COLUMNS);
    let h = h.clamp(1, MAX_H);
    let last = u32::from(MAX_ROWS - h);
    let mut ys: Vec<u32> = placed
        .iter()
        .map(Rect::bottom)
        .filter(|&y| y <= last)
        .collect();
    ys.push(0);
    ys.sort_unstable();
    ys.dedup();
    let mut xs: Vec<u32> = placed
        .iter()
        .map(Rect::right)
        .filter(|&x| x + u32::from(w) <= u32::from(COLUMNS))
        .collect();
    xs.push(0);
    xs.sort_unstable();
    xs.dedup();
    let at = |x: u32, y: u32| Rect {
        x: x as u16,
        y: y as u16,
        w,
        h,
    };
    for &y in &ys {
        for &x in &xs {
            let candidate = at(x, y);
            if !placed.iter().any(|p| p.collides(&candidate)) {
                return candidate;
            }
        }
    }
    at(0, last)
}

/// Place items in order, each in the first free slot: the old dense
/// auto-fill flow, used to migrate boards saved before the canvas.
pub fn flow(sizes: &[(u16, u16)]) -> Vec<Rect> {
    let mut placed = Vec::with_capacity(sizes.len());
    for &(w, h) in sizes {
        let rect = first_free(&placed, w, h);
        placed.push(rect);
    }
    placed
}

/// Why a layout was refused: the index of the offending item.
#[derive(Debug, PartialEq, Eq)]
pub enum Invalid {
    Bounds(usize),
    Overlap(usize),
}

/// Check bounds and overlap, then move every item up as far as it goes
/// (in `y`, then `x` order), exactly like the client's compaction. Each
/// item lands directly below the lowest already-placed item sharing a
/// column with it, so the cost is O(n²) whatever the `y` values.
pub fn compact(rects: &mut [Rect]) -> Result<(), Invalid> {
    if let Some(i) = rects.iter().position(|r| !r.in_bounds()) {
        return Err(Invalid::Bounds(i));
    }
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by_key(|&i| (rects[i].y, rects[i].x, i));
    for (n, &i) in order.iter().enumerate() {
        if order[..n].iter().any(|&j| rects[j].collides(&rects[i])) {
            return Err(Invalid::Overlap(i));
        }
    }
    let mut placed: Vec<Rect> = Vec::with_capacity(rects.len());
    for i in order {
        let mut rect = rects[i];
        // Every placed item started at or above this one without
        // overlapping it and only moved up, so each one sharing a column
        // ends at or above `rect.y`: the landing row is the lowest end.
        let landing = placed
            .iter()
            .filter(|p| p.spans_with(&rect))
            .map(Rect::bottom)
            .max()
            .unwrap_or(0);
        rect.y = landing as u16;
        rects[i] = rect;
        placed.push(rect);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn flow_fills_rows_densely_in_order() {
        let placed = flow(&[(4, 2), (8, 1), (4, 1), (12, 1), (3, 1)]);
        assert_eq!(
            placed,
            vec![
                r(0, 0, 4, 2),
                r(4, 0, 8, 1),
                r(4, 1, 4, 1),
                r(0, 2, 12, 1),
                r(8, 1, 3, 1)
            ]
        );
    }

    #[test]
    fn legacy_sizes_keep_the_old_intent() {
        assert_eq!(legacy_size("container", 4, 1), (4, 2));
        assert_eq!(legacy_size("link", 8, 1), (8, 1));
        assert_eq!(legacy_size("widget", 12, 2), (12, 5));
        assert_eq!(legacy_size("container", 12, 12), (12, 8));
    }

    #[test]
    fn compaction_closes_gaps_and_refuses_overlap_and_bounds() {
        // A full-width item is a floor: nothing below it passes through.
        let mut rects = [r(0, 3, 4, 2), r(4, 9, 4, 1), r(0, 6, 12, 1), r(4, 4, 8, 1)];
        compact(&mut rects).unwrap();
        assert_eq!(
            rects,
            [r(0, 0, 4, 2), r(4, 3, 4, 1), r(0, 2, 12, 1), r(4, 0, 8, 1)]
        );
        let mut overlapping = [r(0, 0, 4, 2), r(2, 1, 4, 2)];
        assert_eq!(compact(&mut overlapping), Err(Invalid::Overlap(1)));
        // Overlap is checked before compaction moves anything.
        let mut later = [r(0, 5, 4, 2), r(2, 6, 4, 2)];
        assert_eq!(compact(&mut later), Err(Invalid::Overlap(1)));
        let mut wide = [r(10, 0, 4, 1)];
        assert_eq!(compact(&mut wide), Err(Invalid::Bounds(0)));
        let mut flat = [r(0, 0, 4, 0)];
        assert_eq!(compact(&mut flat), Err(Invalid::Bounds(0)));
    }

    #[test]
    fn extreme_values_are_refused_without_overflow() {
        let max = u16::MAX;
        for rect in [
            r(max, 0, 1, 1),
            r(11, 0, max, 1),
            r(max - 3, 0, 4, 1),
            r(0, max, 4, 1),
            r(0, max - 1, 4, 16),
            r(0, MAX_ROWS, 4, 1),
            r(0, 0, 4, max),
            r(12, 0, 1, 1),
        ] {
            assert!(!rect.in_bounds(), "{rect:?}");
            assert_eq!(compact(&mut [rect]), Err(Invalid::Bounds(0)));
        }
        assert!(r(0, MAX_ROWS - 16, 12, 16).in_bounds());
        // Collision maths on unvalidated input never overflows.
        assert!(r(max, max, max, max).collides(&r(max, max, max, max)));
        assert!(!r(0, 0, 1, 1).collides(&r(max, max, max, max)));
        let free = first_free(&[r(max, max, max, max)], 4, 2);
        assert_eq!((free.x, free.y), (0, 0));
    }

    #[test]
    fn compaction_cost_does_not_grow_with_y() {
        // The deepest allowed items, many of them: landing rows are computed
        // directly, so this is instant rather than y × n² steps.
        let mut rects: Vec<Rect> = (0..512u16)
            .map(|i| r(i % 12, MAX_ROWS - 16 - (i / 12) * 16, 1, 16))
            .collect();
        let started = std::time::Instant::now();
        compact(&mut rects).unwrap();
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert!(rects
            .iter()
            .all(|rect| rect.y % 16 == 0 && rect.y < 16 * 43));
        let mut stacked = [r(0, 8000, 4, 2), r(2, 8100, 4, 1)];
        compact(&mut stacked).unwrap();
        assert_eq!(stacked, [r(0, 0, 4, 2), r(2, 2, 4, 1)]);
    }
}
