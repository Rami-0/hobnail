//! Line diff (Myers, O((N+M)·D)) for "revision vs current".
//!
//! The common prefix and suffix are trimmed first (revisions usually differ
//! in a few lines). The edit script is searched up to `MAX_D` edits; beyond
//! that the middle part is reported as "all removed, all added" and the
//! result is flagged `approximate` (still a correct diff, just not minimal).
//! Memory is O(D²) for the trace, bounded by `MAX_D`.

use serde_json::{json, Value as J};

/// Largest input per side (bytes and lines).
pub const MAX_BYTES: usize = 512 * 1024;
pub const MAX_LINES: usize = 20_000;
const MAX_D: usize = 1000;
const CONTEXT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Same line: (old index, new index).
    Eq(usize, usize),
    /// Removed old line.
    Del(usize),
    /// Added new line.
    Ins(usize),
}

/// Edit script turning `a` into `b`, in order. The bool is `approximate`.
pub fn diff<T: PartialEq>(a: &[T], b: &[T]) -> (Vec<Op>, bool) {
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf]
    {
        suf += 1;
    }
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut ops: Vec<Op> = (0..pre).map(|i| Op::Eq(i, i)).collect();
    let (mid, approx) = match myers(am, bm) {
        Some(m) => (m, false),
        None => {
            let mut m: Vec<Op> = (0..am.len()).map(Op::Del).collect();
            m.extend((0..bm.len()).map(Op::Ins));
            (m, true)
        }
    };
    ops.extend(mid.into_iter().map(|o| match o {
        Op::Eq(x, y) => Op::Eq(x + pre, y + pre),
        Op::Del(x) => Op::Del(x + pre),
        Op::Ins(y) => Op::Ins(y + pre),
    }));
    let (ao, bo) = (a.len() - suf, b.len() - suf);
    ops.extend((0..suf).map(|i| Op::Eq(ao + i, bo + i)));
    (ops, approx)
}

/// Myers' greedy algorithm with a trace for backtracking. `None` when more
/// than `MAX_D` edits are needed.
fn myers<T: PartialEq>(a: &[T], b: &[T]) -> Option<Vec<Op>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    let limit = max.min(MAX_D);
    let off = max as isize + 1;
    let mut v = vec![0isize; 2 * max + 3];
    // trace[d] = v[k] for k in -(d+1)..=(d+1), as it was before step d.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'outer: for d in 0..=limit as isize {
        trace.push(
            (-(d + 1)..=(d + 1))
                .map(|k| v[(k + off) as usize])
                .collect(),
        );
        let mut k = -d;
        while k <= d {
            let down = k == -d || (k != d && v[(k - 1 + off) as usize] < v[(k + 1 + off) as usize]);
            let mut x = if down {
                v[(k + 1 + off) as usize]
            } else {
                v[(k - 1 + off) as usize] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(k + off) as usize] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'outer;
            }
            k += 2;
        }
    }
    found?;
    let get = |d: isize, k: isize| -> isize {
        let row = &trace[d as usize];
        let i = k + d + 1;
        if i < 0 || i as usize >= row.len() {
            -1
        } else {
            row[i as usize]
        }
    };
    let (mut x, mut y) = (n, m);
    let mut rev = Vec::new();
    for d in (0..trace.len() as isize).rev() {
        let k = x - y;
        let down = k == -d || (k != d && get(d, k - 1) < get(d, k + 1));
        let pk = if down { k + 1 } else { k - 1 };
        let px = get(d, pk);
        let py = px - pk;
        while x > px && y > py {
            x -= 1;
            y -= 1;
            rev.push(Op::Eq(x as usize, y as usize));
        }
        if d > 0 {
            if x == px {
                rev.push(Op::Ins(py as usize));
            } else {
                rev.push(Op::Del(px as usize));
            }
        }
        x = px;
        y = py;
    }
    rev.reverse();
    Some(rev)
}

/// JSON hunks with `CONTEXT` lines of context, like a unified diff.
/// `{"added","removed","approximate","hunks":[{"old_start","old_lines",
/// "new_start","new_lines","lines":[{"op":" "|"-"|"+","text"}]}]}`
/// (line numbers 1-based).
pub fn hunks_json(old: &str, new: &str) -> J {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let (ops, approx) = diff(&a, &b);
    let added = ops.iter().filter(|o| matches!(o, Op::Ins(_))).count();
    let removed = ops.iter().filter(|o| matches!(o, Op::Del(_))).count();
    // Indices of ops that are changes; group with context.
    let changes: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, o)| !matches!(o, Op::Eq(..)))
        .map(|(i, _)| i)
        .collect();
    let mut hunks = Vec::new();
    let mut i = 0;
    while i < changes.len() {
        let start = changes[i].saturating_sub(CONTEXT);
        let mut end = changes[i];
        while i + 1 < changes.len() && changes[i + 1] <= end + 2 * CONTEXT + 1 {
            i += 1;
            end = changes[i];
        }
        let end = (end + CONTEXT).min(ops.len() - 1);
        let slice = &ops[start..=end];
        let (mut os, mut ns) = (None, None);
        let (mut ol, mut nl) = (0, 0);
        let mut lines = Vec::new();
        for o in slice {
            match *o {
                Op::Eq(x, y) => {
                    os.get_or_insert(x);
                    ns.get_or_insert(y);
                    ol += 1;
                    nl += 1;
                    lines.push(json!({"op": " ", "text": a[x]}));
                }
                Op::Del(x) => {
                    os.get_or_insert(x);
                    ol += 1;
                    lines.push(json!({"op": "-", "text": a[x]}));
                }
                Op::Ins(y) => {
                    ns.get_or_insert(y);
                    nl += 1;
                    lines.push(json!({"op": "+", "text": b[y]}));
                }
            }
        }
        // A hunk with no old (or new) lines starts after the line before it.
        let old_start = os
            .map(|x| x + 1)
            .unwrap_or_else(|| prev_line(&ops, start, true));
        let new_start = ns
            .map(|y| y + 1)
            .unwrap_or_else(|| prev_line(&ops, start, false));
        hunks.push(json!({
            "old_start": old_start, "old_lines": ol,
            "new_start": new_start, "new_lines": nl,
            "lines": lines,
        }));
        i += 1;
    }
    json!({"added": added, "removed": removed, "approximate": approx, "hunks": hunks})
}

/// 1-based number of the last old/new line before op `i` (0 if none).
fn prev_line(ops: &[Op], i: usize, old: bool) -> usize {
    ops[..i]
        .iter()
        .rev()
        .find_map(|o| match (*o, old) {
            (Op::Eq(x, _), true) | (Op::Del(x), true) => Some(x + 1),
            (Op::Eq(_, y), false) | (Op::Ins(y), false) => Some(y + 1),
            _ => None,
        })
        .unwrap_or(0)
}
