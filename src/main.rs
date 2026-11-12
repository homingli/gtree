//! gtree: git commit graph focused on branches, worktrees and upstream state.

mod git;
mod graph;
mod render;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "gtree",
    version,
    about = "Git commit graph focused on branches, worktrees and upstream state"
)]
struct Args {
    /// Repository path (defaults to the current directory)
    path: Option<PathBuf>,
    /// Show only the history of this branch/ref instead of all refs
    #[arg(short = 'b', long)]
    branch: Option<String>,
    /// Show only the newest N commits (0 = unlimited)
    #[arg(short = 'n', long)]
    max_count: Option<usize>,
    /// Use ASCII graph characters instead of Unicode box drawing
    #[arg(long)]
    ascii: bool,
    /// Force colored output even when not writing to a terminal
    #[arg(long)]
    color: bool,
    /// Disable colored output
    #[arg(long, conflicts_with = "color")]
    no_color: bool,
    /// Hide the worktree summary
    #[arg(long)]
    no_worktrees: bool,
    /// Hide upstream tracking annotations
    #[arg(long)]
    no_upstream: bool,
}

fn main() {
    let args = Args::parse();
    let repo = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    if let Err(e) = run(&args, &repo) {
        eprintln!("gtree: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args, repo: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !git::is_repo(repo) {
        return Err(format!("{} is not a git repository", repo.display()).into());
    }
    let max_count = args.max_count.filter(|&n| n > 0);
    let data = git::collect(repo, max_count, args.branch.as_deref())?;

    let stdout = std::io::stdout();
    let tty = stdout.is_terminal();
    let style = render::Style {
        color: match (args.color, args.no_color) {
            (true, _) => true,
            (_, true) => false,
            _ => tty && std::env::var_os("NO_COLOR").is_none(),
        },
        ascii: args.ascii || !tty,
    };

    let mut w = std::io::BufWriter::new(stdout.lock());
    if !args.no_worktrees && !data.worktrees.is_empty() {
        render::write_worktrees(&mut w, &style, &data)?;
    }

    if data.commits.is_empty() {
        writeln!(w, "{}", style.dim("No commits yet."))?;
        return Ok(());
    }

    let mut lanes = graph::Lanes::new();
    for commit in &data.commits {
        for row in lanes.layout(commit) {
            let prefix = render::graph_prefix(&style, &row).trim_end().to_string();
            if row.commit {
                let line = render::commit_line(&style, commit, &data, !args.no_upstream);
                writeln!(w, "{prefix} {line}")?;
            } else {
                writeln!(w, "{prefix}")?;
            }
        }
    }
    if data.truncated && lanes.width() > 0 {
        render::write_tail(&mut w, &style, &lanes, data.commits.len())?;
    }
    Ok(())
}
