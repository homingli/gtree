//! Functional tests: run the gtree binary against real temp repositories.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let dir = std::env::temp_dir().join(format!("gtree-it-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git should be installed");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn run_gtree(args: &[&str], cwd: &Path) -> (String, String, Option<i32>) {
    let out = Command::new(env!("CARGO_BIN_EXE_gtree"))
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "")
        .output()
        .expect("gtree binary should run");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

fn commit_file(repo: &Path, name: &str, msg: &str) {
    std::fs::write(repo.join(name), format!("{name}\n")).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", msg]);
}

/// Repo shape: main (c1 <- c3, merge) + feature (c2), tag v1.0 on the merge,
/// origin/main at c3 so main is [ahead 2] (merge + c2), a second worktree on
/// feature, and a `stray` branch whose tip is not merged into main.
fn sample_repo(name: &str) -> PathBuf {
    let dir = unique_dir(name);
    git(&dir, &["init", "-q", "-b", "main"]);
    commit_file(&dir, "a.txt", "c1");
    git(&dir, &["checkout", "-q", "-b", "feature"]);
    commit_file(&dir, "b.txt", "c2-feature");
    git(&dir, &["checkout", "-q", "main"]);
    commit_file(&dir, "c.txt", "c3-main");
    git(&dir, &["merge", "-q", "--no-ff", "feature", "-m", "merge feature"]);
    git(&dir, &["tag", "v1.0"]);
    let c3 = git(&dir, &["rev-parse", "HEAD~1"]);
    git(
        &dir,
        &["update-ref", "refs/remotes/origin/main", c3.trim()],
    );
    // Wire up tracking without a real remote: exactly what
    // `branch --set-upstream-to` would persist in config.
    git(&dir, &["config", "remote.origin.url", "/fake"]);
    git(
        &dir,
        &["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"],
    );
    git(&dir, &["config", "branch.main.remote", "origin"]);
    git(&dir, &["config", "branch.main.merge", "refs/heads/main"]);
    git(&dir, &["worktree", "add", "-q", "wt2", "feature"]);
    // A branch whose tip never made it into main.
    git(&dir, &["checkout", "-q", "-b", "stray", "main~1"]);
    commit_file(&dir, "d.txt", "stray-work");
    git(&dir, &["checkout", "-q", "main"]);
    dir
}

fn commit_lines(out: &str) -> Vec<&str> {
    out.lines().filter(|l| l.contains(" - ")).collect()
}

#[test]
fn shows_topology_decorations_worktrees_and_upstream() {
    let dir = sample_repo("main");
    let (out, _, code) = run_gtree(&["--ascii"], &dir);
    assert_eq!(code, Some(0));

    // Graph topology: merge commit, both parents, the fork point, stray tip.
    assert_eq!(commit_lines(&out).len(), 5, "out: {out}");
    assert!(out.contains("merge feature"), "out: {out}");
    assert!(out.contains("c2-feature"), "out: {out}");
    assert!(out.contains("|/"), "merge bend missing, out: {out}");
    assert!(out.contains("|\\") || out.contains("|/"), "out: {out}");

    // Decorations: HEAD, branches, tag, remote ref.
    assert!(out.contains("(HEAD -> main"), "out: {out}");
    assert!(out.contains("feature)"), "out: {out}");
    assert!(out.contains("tag: v1.0"), "out: {out}");
    assert!(out.contains("origin/main"), "out: {out}");

    // Upstream tracking is inline on the branch decoration.
    assert!(out.contains("[ahead 2]"), "out: {out}");

    // The stray branch tip is flagged; the merged feature branch is not.
    assert!(out.contains("stray (unmerged)"), "out: {out}");
    assert!(!out.contains("feature (unmerged)"), "out: {out}");

    // Worktree summary marks the current worktree and lists wt2.
    assert!(out.contains("worktrees:"), "out: {out}");
    assert!(out.contains("wt2"), "out: {out}");
    let wt_lines: Vec<&str> = out
        .lines()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .collect();
    assert_eq!(wt_lines.len(), 2, "out: {out}");
    assert!(wt_lines[0].trim_start().starts_with('*'), "out: {out}");

    // Piped output is ASCII, never unicode or colored.
    assert!(!out.contains('●') && !out.contains('│'), "out: {out}");
    assert!(!out.contains("\x1b["), "out: {out}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn max_count_truncates_and_notes_tail() {
    let dir = sample_repo("trunc");
    let (out, _, code) = run_gtree(&["--ascii", "-n", "2"], &dir);
    assert_eq!(code, Some(0));
    assert_eq!(commit_lines(&out).len(), 2, "out: {out}");
    assert!(out.contains("truncated"), "out: {out}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn flags_hide_sections() {
    let dir = sample_repo("flags");
    let (out, _, _) = run_gtree(&["--ascii", "--no-worktrees", "--no-upstream"], &dir);
    assert!(!out.contains("worktrees:"), "out: {out}");
    assert!(!out.contains("[ahead 2]"), "out: {out}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn color_flag_forces_ansi() {
    let dir = sample_repo("color");
    let (out, _, _) = run_gtree(&["--ascii", "--color"], &dir);
    assert!(out.contains("\x1b["), "out: {out}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn branch_flag_limits_history_to_that_ref() {
    let dir = sample_repo("branchsel");

    // main's history: merge, c3, c2, c1 — stray is not reachable from main.
    let (out, _, code) = run_gtree(&["--ascii", "-b", "main"], &dir);
    assert_eq!(code, Some(0));
    assert_eq!(commit_lines(&out).len(), 4, "out: {out}");
    assert!(out.contains("merge feature"), "out: {out}");
    assert!(!out.contains("stray-work"), "out: {out}");

    // feature's history stops at its fork point: c2, c1 only.
    let (out, _, code) = run_gtree(&["--ascii", "-b", "feature"], &dir);
    assert_eq!(code, Some(0));
    assert_eq!(commit_lines(&out).len(), 2, "out: {out}");
    assert!(out.contains("c2-feature"), "out: {out}");
    assert!(!out.contains("c3-main"), "out: {out}");

    // The selected branch's tip is still decorated (HEAD is not on it).
    assert!(out.contains("(feature)"), "out: {out}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unknown_branch_fails_with_message() {
    let dir = sample_repo("badbranch");
    let (_, stderr, code) = run_gtree(&["--ascii", "-b", "nope"], &dir);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("unknown revision 'nope'"), "stderr: {stderr}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn branch_glob_shows_union_of_all_matches() {
    let dir = sample_repo("glob");
    git(&dir, &["checkout", "-q", "-b", "topic/a", "main~1"]);
    commit_file(&dir, "ta.txt", "topic-a-work");
    git(&dir, &["checkout", "-q", "main"]);
    git(&dir, &["checkout", "-q", "-b", "topic/b", "main~1"]);
    commit_file(&dir, "tb.txt", "topic-b-work");
    git(&dir, &["checkout", "-q", "main"]);

    let (out, _, code) = run_gtree(&["--ascii", "-b", "topic/*"], &dir);
    assert_eq!(code, Some(0));
    assert!(out.contains("topic-a-work"), "out: {out}");
    assert!(out.contains("topic-b-work"), "out: {out}");
    assert!(out.contains("(topic/a"), "out: {out}");
    assert!(out.contains("(topic/b"), "out: {out}");
    // Common ancestry is included once; unrelated branches are not.
    assert!(out.contains("c3-main"), "out: {out}");
    assert!(!out.contains("stray-work"), "out: {out}");

    // `?` matches exactly one char.
    let (out, _, code) = run_gtree(&["--ascii", "-b", "topi?/a"], &dir);
    assert_eq!(code, Some(0));
    assert!(out.contains("topic-a-work"), "out: {out}");
    assert!(!out.contains("topic-b-work"), "out: {out}");

    // Non-glob args still fall back to git's own resolution.
    let (out, _, code) = run_gtree(&["--ascii", "-b", "HEAD"], &dir);
    assert_eq!(code, Some(0));
    assert!(out.contains("merge feature"), "out: {out}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn branch_glob_with_no_matches_errors() {
    let dir = sample_repo("globnone");
    let (_, stderr, code) = run_gtree(&["--ascii", "-b", "nope/*"], &dir);
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("no branches match 'nope/*'"),
        "stderr: {stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn empty_repo_reports_no_commits() {
    let dir = unique_dir("empty");
    git(&dir, &["init", "-q", "-b", "main"]);
    let (out, _, code) = run_gtree(&["--ascii"], &dir);
    assert_eq!(code, Some(0));
    assert!(out.contains("No commits yet"), "out: {out}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn non_repo_fails_with_message() {
    let dir = unique_dir("norepo");
    let (out, stderr, code) = run_gtree(&["--ascii"], &dir);
    assert_eq!(code, Some(1));
    assert!(out.is_empty());
    assert!(stderr.contains("not a git repository"), "stderr: {stderr}");
    std::fs::remove_dir_all(&dir).ok();
}
