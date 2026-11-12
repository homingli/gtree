//! Lane allocation: turns a topo-ordered commit list into graph rows,
//! using the classic approach from git's `graph.c` (simplified).
//!
//! Rendering scheme matches git: each lane is two characters wide. Nodes and
//! verticals sit at even cells (`2 * lane`), bend glyphs at odd cells between
//! two lanes (`2 * lane + 1`), so bends never collide with verticals.

use crate::git::Commit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Node,
    Vert,
    /// Edge stepping one lane to the right while descending.
    BendRight,
    /// Edge stepping one lane to the left while descending.
    BendLeft,
    /// New parent lane opened on the commit row (end of the connector run).
    BranchOut,
    /// New parent lane passed through by the connector run (octopus merges).
    Tee,
    /// Existing lane crossed by the connector run.
    Cross,
    /// Horizontal connector segment.
    Horizon,
    Blank,
}

#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub glyph: Glyph,
    /// Lane palette index, for coloring the glyph.
    pub color: usize,
}

#[derive(Debug, Clone)]
pub struct GraphRow {
    pub cells: Vec<Cell>,
    /// True for rows carrying a commit node.
    pub commit: bool,
}

struct Edge {
    #[allow(dead_code)]
    oid: String,
    /// Lane the edge settles into.
    tgt: usize,
    /// Lane the edge currently passes through while settling.
    cur: usize,
}

fn blank_row(width: usize) -> Vec<Cell> {
    vec![Cell { glyph: Glyph::Blank, color: 0 }; width]
}

fn vert(lanes_color: usize) -> Cell {
    Cell {
        glyph: Glyph::Vert,
        color: lanes_color,
    }
}

fn blank() -> Cell {
    Cell {
        glyph: Glyph::Blank,
        color: 0,
    }
}

/// Ordered active lanes. Each lane expects one commit oid to appear next;
/// all oids are distinct by construction (a parent already claimed elsewhere bends in).
pub struct Lanes {
    columns: Vec<Option<String>>,
    colors: Vec<usize>,
    next_color: usize,
}

impl Lanes {
    pub fn new() -> Self {
        Lanes {
            columns: Vec::new(),
            colors: Vec::new(),
            next_color: 0,
        }
    }

    pub fn width(&self) -> usize {
        self.columns.len()
    }

    pub fn colors(&self) -> &[usize] {
        &self.colors
    }

    /// Lay out one commit; returns its commit row plus any edge/slide rows.
    pub fn layout(&mut self, commit: &Commit) -> Vec<GraphRow> {
        let col = match self.columns.iter().position(|c| c.as_deref() == Some(&commit.id)) {
            Some(i) => i,
            None => {
                self.columns.push(Some(commit.id.clone()));
                self.colors.push(self.next_color);
                self.next_color += 1;
                self.columns.len() - 1
            }
        };
        let old_len = self.columns.len();

        // Assign each parent to a lane: claimed elsewhere -> bend in,
        // else reuse this commit's slot (first taker only), else open a new
        // lane immediately right of the commit's edge cluster.
        let mut edges: Vec<Edge> = Vec::new();
        let mut col_reused = false;
        let mut rightmost = col;
        for p in &commit.parents {
            if edges.iter().any(|e| e.oid == *p) {
                continue;
            }
            if let Some(j) = self.columns.iter().position(|c| c.as_deref() == Some(p)) {
                edges.push(Edge {
                    oid: p.clone(),
                    tgt: j,
                    cur: col,
                });
            } else if !col_reused {
                col_reused = true;
                self.columns[col] = Some(p.clone());
                edges.push(Edge {
                    oid: p.clone(),
                    tgt: col,
                    cur: col,
                });
            } else {
                let pos = rightmost + 1;
                self.columns.insert(pos, Some(p.clone()));
                self.colors.insert(pos, self.next_color);
                self.next_color += 1;
                for e in edges.iter_mut() {
                    if e.tgt >= pos {
                        e.tgt += 1;
                    }
                }
                // New lanes are drawn on the commit row's connector, so the
                // edge starts out already arrived.
                edges.push(Edge {
                    oid: p.clone(),
                    tgt: pos,
                    cur: pos,
                });
                rightmost = pos;
            }
        }

        let mut rows = Vec::new();

        // Commit row: node at the commit's lane, other lanes vertical. New
        // parent lanes open right here with a connector run: `●─╮`, `┬` for
        // additional lanes, `┼` where the run crosses an existing lane.
        let last = self.columns.len() - 1;
        let has_new = self.columns.len() > old_len;
        let mut cells = Vec::with_capacity(2 * self.columns.len());
        for j in 0..self.columns.len() {
            if j < old_len {
                let cell = if j == col {
                    Cell {
                        glyph: Glyph::Node,
                        color: self.colors[col],
                    }
                } else if has_new && j > col {
                    Cell {
                        glyph: Glyph::Cross,
                        color: self.colors[j],
                    }
                } else {
                    vert(self.colors[j])
                };
                cells.push(cell);
            } else {
                let glyph = if j == last {
                    Glyph::BranchOut
                } else {
                    Glyph::Tee
                };
                cells.push(Cell {
                    glyph,
                    color: self.colors[j],
                });
            }
            if has_new && j >= col && j < last {
                cells.push(Cell {
                    glyph: Glyph::Horizon,
                    color: self.colors[last],
                });
            } else {
                cells.push(blank());
            }
        }
        rows.push(GraphRow { cells, commit: true });

        // Special case, matching git: the single parent edge bends into the
        // adjacent right lane while this commit's own lane is vacated. The two
        // movements cancel into one `|/` row and the target lane slides left.
        let collapse = !col_reused
            && edges.len() == 1
            && edges[0].tgt == col + 1
            && edges[0].cur == col;
        if collapse {
            let j = edges[0].tgt;
            let color = self.colors[j];
            let mut cells = blank_row(2 * self.columns.len());
            cells[2 * col] = vert(color);
            cells[2 * col + 1] = Cell {
                glyph: Glyph::BendLeft,
                color,
            };
            // Lanes further right stay vertical through this row.
            for k in (col + 2)..self.columns.len() {
                cells[2 * k] = vert(self.colors[k]);
            }
            rows.push(GraphRow { cells, commit: false });
            self.columns.remove(col);
            self.colors.remove(col);
            return rows;
        }

        // Settle rows: move every parent edge one lane per row until it
        // reaches its target. Diagonals travel on odd cells, so they never
        // collide with a lane's vertical.
        let cap = 2 * self.columns.len() * edges.len().max(1) + 8;
        for _ in 0..cap {
            if edges.iter().all(|e| e.cur == e.tgt) {
                break;
            }
            let len = self.columns.len();
            let mut cells = blank_row(2 * len);
            for j in 0..len {
                let settled = (j < old_len && (j != col || col_reused))
                    || edges.iter().any(|e| e.tgt == j && e.cur == j);
                if settled {
                    cells[2 * j] = vert(self.colors[j]);
                }
            }
            // Shorter edges claim their cells first so crossing bends stagger.
            let mut order: Vec<usize> = (0..edges.len())
                .filter(|&i| edges[i].cur != edges[i].tgt)
                .collect();
            order.sort_by_key(|&i| edges[i].tgt.abs_diff(edges[i].cur));
            for &i in &order {
                let (cur, tgt) = (edges[i].cur, edges[i].tgt);
                // cur >= 1 whenever the edge moves left (targets are >= 0).
                let (odd_cell, dir, next) = if tgt > cur {
                    (2 * cur + 1, Glyph::BendRight, cur + 1)
                } else {
                    (2 * (cur - 1) + 1, Glyph::BendLeft, cur - 1)
                };
                let is_bend = |g: Glyph| g == Glyph::BendRight || g == Glyph::BendLeft;
                if is_bend(cells[odd_cell].glyph) {
                    continue; // another moving edge drew here this row; retry next row
                }
                cells[odd_cell] = Cell {
                    glyph: dir,
                    color: self.colors[tgt],
                };
                edges[i].cur = next;
            }
            rows.push(GraphRow { cells, commit: false });
        }

        // The commit's slot was not reused: slide lanes right of the hole
        // leftward, one lane per row, then drop the hole.
        if !col_reused {
            while self.columns.len() > col + 1 {
                let len = self.columns.len();
                let mut cells = blank_row(2 * len);
                for j in 0..col {
                    cells[2 * j] = vert(self.colors[j]);
                }
                for j in col + 1..len {
                    cells[2 * j - 1] = Cell {
                        glyph: Glyph::BendLeft,
                        color: self.colors[j],
                    };
                }
                rows.push(GraphRow { cells, commit: false });
                self.columns.remove(col);
                self.colors.remove(col);
            }
            self.columns.pop();
            self.colors.pop();
        }

        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, parents: &[&str]) -> Commit {
        Commit {
            id: id.into(),
            short_id: id.into(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            author: String::new(),
            date: String::new(),
            subject: String::new(),
            decorations: Vec::new(),
        }
    }

    fn draw(commits: &[Commit]) -> Vec<String> {
        let mut lanes = Lanes::new();
        let mut out = Vec::new();
        for c in commits {
            for row in lanes.layout(c) {
                let line: String = row
                    .cells
                    .iter()
                    .map(|cell| match cell.glyph {
                        Glyph::Node => '*',
                        Glyph::Vert => '|',
                        Glyph::BendRight => '\\',
                        Glyph::BendLeft => '/',
                        Glyph::BranchOut => '.',
                        Glyph::Tee => '-',
                        Glyph::Cross => '+',
                        Glyph::Horizon => '-',
                        Glyph::Blank => ' ',
                    })
                    .collect();
                out.push(line.trim_end().to_string());
            }
        }
        out
    }

    #[test]
    fn linear_history_is_stacked_nodes() {
        let rows = draw(&[c("A", &["B"]), c("B", &["C"]), c("C", &[])]);
        assert_eq!(rows, vec!["*", "*", "*"]);
    }

    #[test]
    fn branch_and_merge() {
        // M merges B and F, F branches off B — the canonical git example.
        // The connector `*-.` opens F's lane on the commit row itself.
        let rows = draw(&[c("M", &["B", "F"]), c("F", &["B"]), c("B", &[])]);
        assert_eq!(rows, vec!["*-.", "| *", "|/", "*"]);
    }

    #[test]
    fn merge_into_adjacent_right_lane_collapses() {
        // f1 rides lane 1 straight into probe; m1 (lane 0) also targets probe,
        // so its edge and the lane slide cancel into a single `|/` — same as git.
        let rows = draw(&[
            c("M", &["m1", "f1"]),
            c("f1", &["probe"]),
            c("m1", &["probe"]),
            c("probe", &[]),
        ]);
        assert_eq!(rows, vec!["*-.", "| *", "* |", "|/", "*"]);
    }

    #[test]
    fn octopus_merge() {
        // M opens lanes for B and C on the commit row; after each root the
        // vacated lanes slide left.
        let rows = draw(&[
            c("M", &["A", "B", "C"]),
            c("A", &[]),
            c("B", &[]),
            c("C", &[]),
        ]);
        assert_eq!(rows, vec!["*---.", "* | |", " / /", " /", "*", "*"]);
    }

    #[test]
    fn merge_from_outer_lane_bends_stepwise() {
        // W sits two lanes right of its parent X, so its edge needs two
        // bend rows before the lanes compact.
        let rows = draw(&[
            c("M", &["X", "Y"]),
            c("W", &["X"]),
            c("Y", &["X"]),
            c("X", &[]),
        ]);
        assert_eq!(
            rows,
            vec!["*-.", "| | *", "| |/", "|/|", "| *", "|/", "*"]
        );
    }
}
