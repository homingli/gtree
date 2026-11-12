//! Repository data collection by shelling out to `git` with machine-parseable formats.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Field separator for `--pretty=format` records.
const FIELD: char = '\x1f';
/// Record separator for `--pretty=format` records.
const RECORD: char = '\x1e';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Head,
    Local,
    Remote,
    Tag,
    Stash,
}

#[derive(Debug, Clone)]
pub struct Decoration {
    pub kind: RefKind,
    /// Branch or ref name as displayed ("main", "origin/main", "v1.0", "HEAD").
    pub name: String,
    /// For `HEAD -> branch`, the branch HEAD points at.
    pub head_of: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub id: String,
    pub short_id: String,
    pub parents: Vec<String>,
    pub author: String,
    /// git's `%cr` relative date, e.g. "3 days ago".
    pub date: String,
    pub subject: String,
    pub decorations: Vec<Decoration>,
}

#[derive(Debug, Clone)]
pub struct Upstream {
    pub remote: String,
    /// git's `%(upstream:track,nobracket)`: "" when in sync, "ahead 1, behind 2", "gone".
    pub track: String,
}

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub bare: bool,
}

pub struct RepoData {
    pub commits: Vec<Commit>,
    pub upstreams: HashMap<String, Upstream>,
    pub worktrees: Vec<Worktree>,
    /// Index into `worktrees` of the worktree the command runs in.
    pub current_worktree: Option<usize>,
    /// Branch (local and remote) tips that are not merged into HEAD.
    pub unmerged: HashSet<String>,
    /// True when `--max-count` cut older history.
    pub truncated: bool,
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn is_repo(repo: &Path) -> bool {
    git(repo, &["rev-parse", "--git-dir"]).is_ok()
}

fn is_no_commits(err: &str) -> bool {
    err.contains("does not have any commits yet")
        || err.contains("bad object HEAD")
        || err.contains("ambiguous argument")
}

fn is_unknown_revision(err: &str) -> bool {
    err.contains("unknown revision") || err.contains("ambiguous argument")
}

fn has_glob(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

/// Shell-style glob over ref short names: `*` matches any run (including
/// empty), `?` exactly one char, everything else literal.
fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star_p, mut star_n) = (None, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_p = Some(pi);
            star_n = ni;
            pi += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_n += 1;
            ni = star_n;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Expand a glob pattern against branch, remote-ref and tag short names.
/// Sorted alphabetically (for-each-ref order); empty when nothing matches.
fn expand_ref_glob(repo: &Path, pattern: &str) -> Result<Vec<String>, String> {
    let out = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty() && glob_match(pattern, name))
        .map(String::from)
        .collect())
}

pub fn collect(repo: &Path, max_count: Option<usize>, branch: Option<&str>) -> Result<RepoData, String> {
    let mut upstreams = HashMap::new();
    let mut local_branches = HashSet::new();
    // for-each-ref has no %xNN escapes (unlike --pretty), so tab-separated
    // fields: ref names cannot contain tabs.
    if let Ok(out) = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)\t%(upstream:short)\t%(upstream:track,nobracket)",
            "refs/heads",
        ],
    ) {
        for line in out.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.is_empty() || f[0].is_empty() {
                continue;
            }
            local_branches.insert(f[0].to_string());
            if f.len() >= 2 && !f[1].is_empty() {
                upstreams.insert(
                    f[0].to_string(),
                    Upstream {
                        remote: f[1].to_string(),
                        track: f.get(2).map(|s| s.trim().to_string()).unwrap_or_default(),
                    },
                );
            }
        }
    }

    let worktrees = collect_worktrees(repo);
    let current_worktree = current_worktree_index(repo, &worktrees);
    let unmerged = collect_unmerged(repo);

    let mut args: Vec<String> = vec![
        "log".into(),
        "--topo-order".into(),
        "--pretty=format:%H%x1f%h%x1f%P%x1f%an%x1f%cr%x1f%s%x1f%d%x1e".into(),
    ];
    match branch {
        Some(pat) => {
            let matched = expand_ref_glob(repo, pat)?;
            if !matched.is_empty() {
                // One graph spanning the union of all matching branches.
                args.extend(matched);
            } else if has_glob(pat) {
                return Err(format!("no branches match '{pat}'"));
            } else {
                // No ref name matched: let git resolve it (SHA, HEAD, tag, …).
                args.push(pat.to_string());
            }
        }
        None => args.push("--all".into()),
    }
    if let Some(n) = max_count {
        args.push(format!("--max-count={n}"));
    }
    let git_args: Vec<&str> = args.iter().map(String::as_str).collect();
    let commits = match git(repo, &git_args) {
        Ok(out) => parse_commits(&out, &local_branches),
        Err(e) if branch.is_some() && is_unknown_revision(&e) => {
            return Err(format!("unknown revision '{}'", branch.unwrap_or("?")));
        }
        Err(e) if is_no_commits(&e) => Vec::new(),
        Err(e) => return Err(e),
    };
    let truncated = match max_count {
        Some(n) => n > 0 && commits.len() == n,
        None => false,
    };

    Ok(RepoData {
        commits,
        upstreams,
        worktrees,
        current_worktree,
        unmerged,
        truncated,
    })
}

/// Branch tips (local and remote) that are not ancestors of HEAD, i.e. whose
/// commits would be lost if the branch were deleted. Best effort: an empty
/// set is fine when HEAD cannot be resolved (empty repo, detached oddities).
fn collect_unmerged(repo: &Path) -> HashSet<String> {
    let mut unmerged = HashSet::new();
    for remote_only in [false, true] {
        let mut args: Vec<&str> = vec!["branch", "--no-merged", "HEAD", "--format=%(refname:short)"];
        if remote_only {
            args.insert(1, "-r");
        }
        if let Ok(out) = git(repo, &args) {
            for line in out.lines() {
                let name = line.trim();
                if !name.is_empty() {
                    unmerged.insert(name.to_string());
                }
            }
        }
    }
    unmerged
}

fn parse_commits(out: &str, local_branches: &HashSet<String>) -> Vec<Commit> {
    out.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches(RECORD);
            if line.is_empty() {
                return None;
            }
            let f: Vec<&str> = line.split(FIELD).collect();
            if f.len() < 7 {
                return None;
            }
            Some(Commit {
                id: f[0].into(),
                short_id: f[1].into(),
                parents: f[2].split_whitespace().map(String::from).collect(),
                author: f[3].into(),
                date: f[4].into(),
                subject: f[5].into(),
                decorations: parse_decorations(f[6], local_branches),
            })
        })
        .collect()
}

fn parse_decorations(s: &str, local_branches: &HashSet<String>) -> Vec<Decoration> {
    let t = s.trim();
    let inner = t
        .strip_prefix('(')
        .and_then(|x| x.strip_suffix(')'))
        .unwrap_or(t);
    if inner.is_empty() {
        return Vec::new();
    }
    inner
        .split(", ")
        .map(|tok| {
            let tok = tok.trim();
            if let Some(branch) = tok.strip_prefix("HEAD -> ") {
                Decoration {
                    kind: RefKind::Head,
                    name: "HEAD".into(),
                    head_of: Some(branch.into()),
                }
            } else if tok == "HEAD" {
                Decoration {
                    kind: RefKind::Head,
                    name: "HEAD".into(),
                    head_of: None,
                }
            } else if let Some(tag) = tok.strip_prefix("tag: ") {
                Decoration {
                    kind: RefKind::Tag,
                    name: tag.into(),
                    head_of: None,
                }
            } else if tok == "stash" {
                Decoration {
                    kind: RefKind::Stash,
                    name: tok.into(),
                    head_of: None,
                }
            } else if local_branches.contains(tok) {
                Decoration {
                    kind: RefKind::Local,
                    name: tok.into(),
                    head_of: None,
                }
            } else {
                Decoration {
                    kind: RefKind::Remote,
                    name: tok.into(),
                    head_of: None,
                }
            }
        })
        .collect()
}

fn collect_worktrees(repo: &Path) -> Vec<Worktree> {
    let Ok(out) = git(repo, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
    let mut wts: Vec<Worktree> = Vec::new();
    let mut cur: Option<Worktree> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                wts.push(w);
            }
            cur = Some(Worktree {
                path: PathBuf::from(p),
                head: None,
                branch: None,
                bare: false,
            });
        } else if let Some(w) = cur.as_mut() {
            if let Some(h) = line.strip_prefix("HEAD ") {
                w.head = Some(h.chars().take(7).collect());
            } else if let Some(b) = line.strip_prefix("branch ") {
                w.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            } else if line == "detached" {
                w.branch = None;
            } else if line == "bare" {
                w.bare = true;
            }
        }
    }
    if let Some(w) = cur.take() {
        wts.push(w);
    }
    wts
}

fn current_worktree_index(repo: &Path, wts: &[Worktree]) -> Option<usize> {
    let repo = std::fs::canonicalize(repo).ok()?;
    wts.iter().position(|w| {
        std::fs::canonicalize(&w.path)
            .map(|p| repo.starts_with(&p))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn glob_matches_wildcards() {
        assert!(glob_match("origin/feature/eng-359-*", "origin/feature/eng-359-a"));
        assert!(glob_match("origin/feature/eng-359-*", "origin/feature/eng-359-a/sub"));
        assert!(glob_match("topi?/a", "topic/a"));
        assert!(glob_match("a*b*c", "aXbYc"));
        assert!(glob_match("*", ""));
        assert!(glob_match("main", "main"));
    }

    #[test]
    fn glob_rejects_non_matches() {
        assert!(!glob_match("feature/*", "origin/feature/x"));
        assert!(!glob_match("feature/*", "feature"));
        assert!(!glob_match("main", "mainx"));
        assert!(!glob_match("topic/*", "topi/a"));
        assert!(!glob_match("", "main"));
    }
}
