//! The graph column of a git history: each commit's lane, the lines from its children above and
//! to its parents below, and the branch tips' names. The commits come children first (`git log
//! --date-order`); rows are worked out top down only as far as a window asks, so the first
//! screenful of a long list costs that screenful. Drawn as SVG by the desktop app and in
//! box-drawing characters by the terminal app (`boxes`).

use serde::Serialize;
use std::sync::Arc;

use crate::history::Commit;

/// Which side of HEAD and its upstream a commit is on, in a list against the upstream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// On HEAD's side only: not pushed.
    Ahead,
    /// On the upstream's only: not merged (as of the last fetch).
    Behind,
}

/// One commit's row. Columns count lanes from the left. A line in the upper half runs from a
/// column at the row's top to one at its middle; one in the lower half from the middle down.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Row {
    /// The commit's id, as long as in its path: a row is drawn only on its own commit's entry.
    pub id: String,
    pub node: u16,
    pub up: Vec<(u16, u16)>,
    pub down: Vec<(u16, u16)>,
    /// What points at the commit, as git names it: `HEAD -> main`, `origin/main`, `tag: v1`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
}

impl Row {
    /// Columns the row uses.
    pub fn width(&self) -> u16 {
        self.up
            .iter()
            .chain(&self.down)
            .flat_map(|&(a, b)| [a, b])
            .chain([self.node])
            .max()
            .unwrap_or(0)
            + 1
    }
}

/// The layout of a list of commits, worked out as far as asked.
pub struct Layout {
    commits: Arc<Vec<Commit>>,
    /// The commit each lane waits for, at the bottom of the last row worked out.
    lanes: Vec<Option<String>>,
    rows: Vec<Row>,
}

impl Layout {
    pub fn new(commits: Arc<Vec<Commit>>) -> Layout {
        Layout {
            commits,
            lanes: vec![],
            rows: vec![],
        }
    }

    pub fn commits(&self) -> &Arc<Vec<Commit>> {
        &self.commits
    }

    /// The rows of commits `from..to` (cut to the list), working out those above first.
    pub fn rows(&mut self, from: usize, to: usize) -> &[Row] {
        let to = to.min(self.commits.len());
        while self.rows.len() < to {
            let row = self.next();
            self.rows.push(row);
        }
        &self.rows[from.min(to)..to]
    }

    fn next(&mut self) -> Row {
        let commits = self.commits.clone();
        let c = &commits[self.rows.len()];
        let lanes = &mut self.lanes;
        let busy: Vec<bool> = lanes.iter().map(Option::is_some).collect();
        let waits = |l: &Option<String>, h: &str| l.as_deref() == Some(h);
        // Its lane: the first that waits for it; a branch tip takes a free one.
        let node = lanes
            .iter()
            .position(|l| waits(l, &c.hash))
            .unwrap_or_else(|| free(lanes, &busy));
        let mut up = vec![];
        let mut through = vec![];
        for (i, l) in lanes.iter_mut().enumerate() {
            match l {
                Some(h) if *h == c.hash => {
                    up.push((i as u16, node as u16));
                    *l = None;
                }
                Some(_) => {
                    up.push((i as u16, i as u16));
                    through.push(i as u16);
                }
                None => {}
            }
        }
        let mut down = vec![];
        for (k, p) in c.parents.iter().enumerate() {
            let to = match lanes.iter().position(|l| waits(l, p)) {
                // A merge's other parent that a lane waits for already: the line joins it.
                Some(j) if k > 0 => j,
                // The first parent goes on in the commit's lane; lanes waiting for the same
                // commit meet at its row.
                _ if k == 0 => node,
                _ => free(lanes, &busy),
            };
            lanes[to] = Some(p.clone());
            down.push((node as u16, to as u16));
        }
        down.extend(through.iter().map(|&i| (i, i)));
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        Row {
            id: c.hash[..c.hash.len().min(crate::history::ID)].to_string(),
            node: node as u16,
            up,
            down,
            refs: c.refs.clone(),
            side: c.side,
        }
    }
}

/// A free lane, not one that was busy at the row's top (a line would run into it and out
/// again): the first, or a new one.
fn free(lanes: &mut Vec<Option<String>>, busy: &[bool]) -> usize {
    match (0..lanes.len()).find(|&i| lanes[i].is_none() && !busy.get(i).copied().unwrap_or(false)) {
        Some(i) => i,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

/// A row in box-drawing characters, two cells a lane (the lane, then the gap to the next),
/// `lanes` lanes wide: `●─╮`, `│ ●`, `├─╯`. The second of each pair is the node's column, so
/// the caller can colour by lane.
pub fn boxes(row: &Row, lanes: u16) -> String {
    const U: u8 = 1;
    const D: u8 = 2;
    const L: u8 = 4;
    const R: u8 = 8;
    let n = lanes as usize * 2;
    let mut cell = vec![0u8; n];
    let span = |a: u16, b: u16, cell: &mut Vec<u8>| {
        let (a, b) = (a as usize * 2, b as usize * 2);
        let (lo, hi) = (a.min(b), a.max(b));
        for (i, c) in cell
            .iter_mut()
            .enumerate()
            .take(hi.min(n.saturating_sub(1)) + 1)
            .skip(lo)
        {
            if i > lo {
                *c |= L;
            }
            if i < hi {
                *c |= R;
            }
        }
    };
    for &(a, b) in &row.up {
        if let Some(c) = cell.get_mut(a as usize * 2) {
            *c |= U;
        }
        span(a, b, &mut cell);
    }
    for &(a, b) in &row.down {
        if let Some(c) = cell.get_mut(b as usize * 2) {
            *c |= D;
        }
        span(a, b, &mut cell);
    }
    cell.iter()
        .enumerate()
        .map(|(i, &f)| {
            if i == row.node as usize * 2 {
                return '●';
            }
            match f {
                0 => ' ',
                x if x == U | D => '│',
                x if x == U => '╵',
                x if x == D => '╷',
                x if x == L | R => '─',
                x if x == L || x == R => '─',
                x if x == U | R => '╰',
                x if x == U | L => '╯',
                x if x == D | R => '╭',
                x if x == D | L => '╮',
                x if x == U | D | R => '├',
                x if x == U | D | L => '┤',
                x if x == U | L | R => '┴',
                x if x == D | L | R => '┬',
                _ => '┼',
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(hash: &str, parents: &[&str]) -> Commit {
        Commit {
            hash: hash.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            ..Commit::default()
        }
    }

    fn draw(commits: Vec<Commit>) -> Vec<String> {
        let n = commits.len();
        let mut l = Layout::new(Arc::new(commits));
        let rows = l.rows(0, n).to_vec();
        let w = rows.iter().map(Row::width).max().unwrap_or(1);
        rows.iter()
            .map(|r| boxes(r, w).trim_end().to_string())
            .collect()
    }

    #[test]
    fn a_line_is_one_lane() {
        assert_eq!(
            draw(vec![c("c", &["b"]), c("b", &["a"]), c("a", &[])]),
            ["●", "●", "●"]
        );
    }

    #[test]
    fn a_merge_opens_a_lane_and_the_fork_closes_it() {
        // m merges s into b; s and b both come from a.
        let rows = draw(vec![
            c("m", &["b", "s"]),
            c("s", &["a"]),
            c("b", &["a"]),
            c("a", &[]),
        ]);
        assert_eq!(rows, ["●─╮", "│ ●", "● │", "●─╯"]);
    }

    #[test]
    fn branch_tips_take_new_lanes() {
        // Two tips, x and y, over a common a.
        let rows = draw(vec![c("x", &["a"]), c("y", &["a"]), c("a", &[])]);
        assert_eq!(rows, ["●", "│ ●", "●─╯"]);
    }

    #[test]
    fn a_parent_beyond_the_list_runs_on_to_the_bottom() {
        let mut l = Layout::new(Arc::new(vec![c("b", &["gone"]), c("a", &[])]));
        let rows = l.rows(0, 2).to_vec();
        assert_eq!(rows[0].down, [(0, 0)]);
        // a is a tip of its own, beside the lane still waiting for "gone".
        assert_eq!((rows[1].node, rows[1].up.clone()), (1, vec![(0, 0)]));
    }

    #[test]
    fn rows_are_worked_out_only_as_far_as_asked() {
        // No work per row that grows with the list (a copy of the commits, a parent looked for
        // by scanning): 8 times the commits cost about 8 times, not 64. Before the long list
        // below, which such work would make take hours.
        let line = |n: usize| -> Arc<Vec<Commit>> {
            Arc::new(
                (0..n)
                    .map(|i| c(&format!("{i:040}"), &[&format!("{:040}", i + 1)]))
                    .collect(),
            )
        };
        let (small, large) = (line(2_000), line(16_000));
        crate::assert_costs_at_most(
            20,
            || assert_eq!(Layout::new(small.clone()).rows(1_950, 2_000).len(), 50),
            || assert_eq!(Layout::new(large.clone()).rows(15_950, 16_000).len(), 50),
        );
        let commits: Vec<Commit> = (0..100_000)
            .map(|i| c(&format!("{i:040}"), &[&format!("{:040}", i + 1)]))
            .collect();
        let mut l = Layout::new(Arc::new(commits));
        assert_eq!(l.rows(0, 50).len(), 50);
        assert_eq!(l.rows.len(), 50);
        // A window at the far end of 100,000 commits: each row worked out once, and each costs
        // a pass over the lanes, which stay as few as the branches (one line: one lane). Lanes
        // that piled up would make it quadratic. Counted, not timed.
        assert_eq!(l.rows(99_950, 100_000).len(), 50);
        assert_eq!(l.rows.len(), 100_000);
        assert!(l.lanes.len() <= 1, "{} lanes", l.lanes.len());
        assert!(l.rows.iter().all(|r| r.up.len() + r.down.len() <= 2));
    }
}
