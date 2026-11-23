//! The board canvas grid: 12 columns of integer cells, a fixed row height
//! chosen by the client. Items are rectangles (`x`, `y`, `w`, `h`) inside
//! one section. The client compacts vertically (gridstack-style gravity);
//! the server checks bounds and overlap and applies the same compaction, so
//! a stored layout never has gaps the client would not have produced.

/// Columns of the canvas.
pub const COLUMNS: u16 = 12;
/// Tallest item, in rows.
pub const MAX_H: u16 = 16;
/// Rows a section may extend to (512 items of one row each fit easily).
pub const MAX_ROWS: u16 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn collides(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    pub fn in_bounds(&self) -> bool {
        (1..=COLUMNS).contains(&self.w)
            && (1..=MAX_H).contains(&self.h)
            && self.x + self.w <= COLUMNS
            && u32::from(self.y) + u32::from(self.h) <= u32::from(MAX_ROWS)
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
pub fn first_free(placed: &[Rect], w: u16, h: u16) -> Rect {
    let w = w.clamp(1, COLUMNS);
    let h = h.clamp(1, MAX_H);
    let mut ys: Vec<u16> = placed.iter().map(|p| p.y + p.h).collect();
    ys.push(0);
    ys.sort_unstable();
    ys.dedup();
    let mut xs: Vec<u16> = placed
        .iter()
        .map(|p| p.x + p.w)
        .filter(|&x| x + w <= COLUMNS)
        .collect();
    xs.push(0);
    xs.sort_unstable();
    xs.dedup();
    for &y in &ys {
        for &x in &xs {
            let candidate = Rect { x, y, w, h };
            if !placed.iter().any(|p| p.collides(&candidate)) {
                return candidate;
            }
        }
    }
    unreachable!("the row below every placed item is free")
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
/// (in `y`, then `x` order), exactly like the client's compaction.
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
        while rect.y > 0 {
            let up = Rect {
                y: rect.y - 1,
                ..rect
            };
            if placed.iter().any(|p| p.collides(&up)) {
                break;
            }
            rect = up;
        }
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
}
