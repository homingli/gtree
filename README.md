# gtree

A local CLI that shows a git repository's commit graph the way
`git log --graph --all --decorate --oneline --color` does, tuned for one
thing: **identifying a repo's branches at a glance**.

On top of the graph it also summarizes worktrees and shows upstream
tracking state inline (`branch→origin/branch [ahead 1, behind 2]`).

## Install

```sh
cargo install --path .
```

## Usage

```sh
gtree                 # current directory
gtree ~/Github/foo    # any repository path
gtree -n 50           # only the newest 50 commits
gtree -b feature/x    # only the history of one branch
gtree -b 'origin/feature/eng-359-*'   # union of every matching branch
```

```text
worktrees:
* /code/video-compressor            main                        1ebd2c1
  /code/video-compressor-async-jobs refactor/async-job-survival 405a6eb

● 1ebd2c1 - (HEAD -> main, origin/main, origin/HEAD) Merge pull request #22 (3 days ago) <HML>
│╰
│ ● 6e90d2c - (chore/dep-hygiene) chore(deps): fix vulnerabilities (3 days ago) <Ho Ming Li>
● 5d3e791 - fix(frontend): allow LAN origins (2 weeks ago) <Ho Ming Li>
```

Branch tips that are **not merged into HEAD** are flagged with a dim
`(unmerged)` suffix — everything else is fully reachable, so a bare
`origin/…` ref is safe to delete with `git push origin --delete <name>`.

Colors follow git's conventions: decoration frame yellow, `HEAD` bold
cyan, local branches bold green, remotes bold red, tags bold yellow, and
one lane color per branch line (red, green, yellow, …, cycling with bold
variants). A merge's second parent opens its lane with a `●─╮` connector
on the merge line itself; joins render as `╯`/`╰` corners. On a terminal
you get Unicode glyphs, when piped plain ASCII (`*-.`, `\`, `/`).

## Options

| Flag | Effect |
| --- | --- |
| `[PATH]` | Repository path (default: current directory) |
| `-b, --branch <REF>` | Show only the history of this branch/ref instead of all refs. `*` and `?` globs match local branches, `origin/…` remote refs and tags; all matches are shown in one graph. Non-glob args fall back to git's own resolution (SHA, `HEAD`, …) |
| `-n, --max-count <N>` | Show only the newest N commits (0 = unlimited, the default) |
| `--ascii` | Force ASCII graph characters |
| `--color` / `--no-color` | Force or disable ANSI colors (auto by default) |
| `--no-worktrees` | Hide the worktree summary |
| `--no-upstream` | Hide `→upstream [ahead/behind]` annotations |

## How it works

`gtree` shells out to `git` for machine-parseable data (`git log
--topo-order` with a control-character-delimited format, `for-each-ref`,
`worktree list --porcelain`), so it honors your git config and behaves
like real git. The lane-allocation algorithm is a simplified port of
git's `graph.c`: two-character lanes, bends on the odd cell between two
lanes, and stepwise slide rows when lanes compact.

## Development

```sh
cargo test    # unit tests for the lane algorithm + functional tests on real temp repos
cargo clippy
```
