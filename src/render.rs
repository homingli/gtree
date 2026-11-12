//! Output rendering: glyphs, ANSI colors, header and commit lines.
//!
//! Decoration colors mirror `git log --decorate --color` defaults:
//! yellow frame, HEAD bold cyan, local branches bold green, remotes bold red,
//! tags bold yellow. Lane colors follow git's graph palette cycle.

use std::io::Write;

use owo_colors::OwoColorize;

use crate::git::{Commit, RefKind, RepoData};
use crate::graph::{Cell, Glyph, GraphRow, Lanes};

pub struct Style {
    pub color: bool,
    pub ascii: bool,
}

const LANE_COLORS: usize = 12;

impl Style {
    fn glyph_str(&self, g: Glyph) -> &'static str {
        match g {
            Glyph::Node => {
                if self.ascii {
                    "*"
                } else {
                    "●"
                }
            }
            Glyph::Vert => {
                if self.ascii {
                    "|"
                } else {
                    "│"
                }
            }
            Glyph::BendRight => {
                if self.ascii {
                    "\\"
                } else {
                    "╰"
                }
            }
            Glyph::BendLeft => {
                if self.ascii {
                    "/"
                } else {
                    "╯"
                }
            }
            Glyph::BranchOut => {
                if self.ascii {
                    "."
                } else {
                    "╮"
                }
            }
            Glyph::Tee => {
                if self.ascii {
                    "-"
                } else {
                    "┬"
                }
            }
            Glyph::Cross => {
                if self.ascii {
                    "+"
                } else {
                    "┼"
                }
            }
            Glyph::Horizon => {
                if self.ascii {
                    "-"
                } else {
                    "─"
                }
            }
            Glyph::Blank => " ",
        }
    }

    /// git's graph lane palette: red, green, yellow, blue, magenta, cyan,
    /// then the bold variants.
    fn lane(&self, idx: usize, s: &str) -> String {
        if !self.color {
            return s.to_owned();
        }
        match idx % LANE_COLORS {
            0 => s.red().to_string(),
            1 => s.green().to_string(),
            2 => s.yellow().to_string(),
            3 => s.blue().to_string(),
            4 => s.magenta().to_string(),
            5 => s.cyan().to_string(),
            6 => s.red().bold().to_string(),
            7 => s.green().bold().to_string(),
            8 => s.yellow().bold().to_string(),
            9 => s.blue().bold().to_string(),
            10 => s.magenta().bold().to_string(),
            _ => s.cyan().bold().to_string(),
        }
    }

    fn hash(&self, s: &str) -> String {
        if self.color {
            s.red().to_string()
        } else {
            s.to_owned()
        }
    }

    fn date(&self, s: &str) -> String {
        if self.color {
            s.green().to_string()
        } else {
            s.to_owned()
        }
    }

    fn author(&self, s: &str) -> String {
        if self.color {
            s.blue().bold().to_string()
        } else {
            s.to_owned()
        }
    }

    /// Decoration frame: parens, commas, the `HEAD ->` arrow.
    fn deco(&self, s: &str) -> String {
        if self.color {
            s.yellow().to_string()
        } else {
            s.to_owned()
        }
    }

    fn head(&self, s: &str) -> String {
        if self.color {
            s.cyan().bold().to_string()
        } else {
            s.to_owned()
        }
    }

    fn local(&self, s: &str) -> String {
        if self.color {
            s.green().bold().to_string()
        } else {
            s.to_owned()
        }
    }

    fn remote(&self, s: &str) -> String {
        if self.color {
            s.red().bold().to_string()
        } else {
            s.to_owned()
        }
    }

    fn tag(&self, s: &str) -> String {
        if self.color {
            s.yellow().bold().to_string()
        } else {
            s.to_owned()
        }
    }

    pub fn dim(&self, s: &str) -> String {
        if self.color {
            s.dimmed().to_string()
        } else {
            s.to_owned()
        }
    }
}

pub fn graph_prefix(style: &Style, row: &GraphRow) -> String {
    row.cells
        .iter()
        .map(|c: &Cell| {
            let g = style.glyph_str(c.glyph);
            if g == " " {
                " ".to_string()
            } else {
                style.lane(c.color, g)
            }
        })
        .collect()
}

pub fn commit_line(style: &Style, c: &Commit, data: &RepoData, show_upstream: bool) -> String {
    let mut line = style.hash(&c.short_id);
    line.push_str(" -");

    let head_branch = c.decorations.iter().find_map(|d| d.head_of.clone());
    let remotes_here: Vec<&str> = c
        .decorations
        .iter()
        .filter(|d| d.kind == RefKind::Remote)
        .map(|d| d.name.as_str())
        .collect();
    let mut items: Vec<String> = Vec::new();
    for d in &c.decorations {
        match d.kind {
            RefKind::Head => {
                let mut text = match &d.head_of {
                    Some(b) => format!(
                        "{}{}{}",
                        style.head("HEAD"),
                        style.deco(" -> "),
                        style.local(b)
                    ),
                    None => style.head("HEAD"),
                };
                if show_upstream {
                    text.push_str(&upstream_suffix(style, data, d.head_of.as_deref(), &remotes_here));
                }
                items.push(text);
            }
            RefKind::Local => {
                if head_branch.as_deref() == Some(d.name.as_str()) {
                    continue; // already shown as part of `HEAD -> branch`
                }
                let mut text = style.local(&d.name);
                if data.unmerged.contains(&d.name) {
                    text.push_str(&style.dim(" (unmerged)"));
                }
                if show_upstream {
                    text.push_str(&upstream_suffix(style, data, Some(&d.name), &remotes_here));
                }
                items.push(text);
            }
            RefKind::Remote => {
                let mut text = style.remote(&d.name);
                if data.unmerged.contains(&d.name) {
                    text.push_str(&style.dim(" (unmerged)"));
                }
                items.push(text);
            }
            RefKind::Tag => items.push(style.tag(&format!("tag: {}", d.name))),
            RefKind::Stash => items.push(style.dim(&d.name)),
        }
    }
    if !items.is_empty() {
        line.push(' ');
        line.push_str(&style.deco("("));
        line.push_str(&items.join(&style.deco(", ")));
        line.push_str(&style.deco(")"));
    }

    line.push(' ');
    line.push_str(&c.subject);
    line.push_str(&format!(" ({})", style.date(&c.date)));
    line.push_str(&format!(" <{}>", style.author(&c.author)));
    line
}

/// `→remote` plus `[ahead/behind]` for a branch's configured upstream.
/// The arrow is skipped when the remote ref is already decorated on this
/// same commit — it would repeat what the parens already show.
fn upstream_suffix(
    style: &Style,
    data: &RepoData,
    branch: Option<&str>,
    remotes_here: &[&str],
) -> String {
    let Some(u) = branch.and_then(|b| data.upstreams.get(b)) else {
        return String::new();
    };
    let mut s = String::new();
    if !remotes_here.contains(&u.remote.as_str()) {
        let arrow = if style.ascii { "->" } else { "→" };
        s.push_str(&style.dim(&format!("{arrow}{}", u.remote)));
    }
    if !u.track.is_empty() {
        s.push_str(&style.dim(&format!(" [{}]", u.track)));
    }
    s
}

pub fn write_worktrees(
    w: &mut impl Write,
    style: &Style,
    data: &RepoData,
) -> std::io::Result<()> {
    writeln!(w, "{}", style.dim("worktrees:"))?;
    let pw = data
        .worktrees
        .iter()
        .map(|x| x.path.display().to_string().len())
        .max()
        .unwrap_or(0);
    let bw = data
        .worktrees
        .iter()
        .map(|x| branch_label(x).len())
        .max()
        .unwrap_or(0);
    for (i, wt) in data.worktrees.iter().enumerate() {
        let marker = if data.current_worktree == Some(i) {
            style.head("*")
        } else {
            " ".to_string()
        };
        let path = format!("{:<pw$}", wt.path.display());
        let label = if wt.bare || wt.branch.is_none() {
            style.dim(&format!("{:<bw$}", branch_label(wt)))
        } else {
            style.local(&format!("{:<bw$}", branch_label(wt)))
        };
        let head = wt.head.clone().unwrap_or_default();
        writeln!(w, "{marker} {} {} {}", path, label, style.hash(&head))?;
    }
    writeln!(w)
}

fn branch_label(wt: &crate::git::Worktree) -> String {
    if wt.bare {
        "(bare)".into()
    } else {
        wt.branch
            .clone()
            .unwrap_or_else(|| "(detached)".into())
    }
}

pub fn write_tail(
    w: &mut impl Write,
    style: &Style,
    lanes: &Lanes,
    shown: usize,
) -> std::io::Result<()> {
    let line: String = lanes
        .colors()
        .iter()
        .map(|&c| style.lane(c, style.glyph_str(Glyph::Vert)))
        .collect();
    writeln!(w, "{}", line.trim_end())?;
    writeln!(
        w,
        "{}",
        style.dim(&format!(
            "… showing newest {shown} commits (older history truncated)"
        ))
    )
}
