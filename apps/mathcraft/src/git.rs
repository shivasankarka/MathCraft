//! Git status for the opened folder, read by running the `git` command (hidden when it is missing or the folder is
//! not a repository). Read-only: it never changes the repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What git says about a file, most worrying last (a folder shows the worst of what is inside it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    Added,
    Untracked,
    Modified,
    Deleted,
    Conflict,
}

impl Mark {
    /// The letter shown beside the file name.
    pub fn letter(self) -> &'static str {
        match self {
            Mark::Added => "A",
            Mark::Untracked => "U",
            Mark::Modified => "M",
            Mark::Deleted => "D",
            Mark::Conflict => "!",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Mark::Added => "added (staged)",
            Mark::Untracked => "new, not tracked yet",
            Mark::Modified => "changed since the last commit",
            Mark::Deleted => "deleted",
            Mark::Conflict => "has a merge conflict",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    /// Changed files, as paths relative to the opened folder.
    pub files: HashMap<PathBuf, Mark>,
    /// The worst mark inside each folder that holds changed files, relative to the opened folder.
    pub dirs: HashMap<PathBuf, Mark>,
}

impl Status {
    pub fn mark(&self, rel: &Path, is_dir: bool) -> Option<Mark> {
        if is_dir { self.dirs.get(rel) } else { self.files.get(rel) }.copied()
    }

    /// "main · 2 changed" for the status bar.
    pub fn summary(&self) -> String {
        let n = self.files.len();
        let mut s = self.branch.clone();
        if self.ahead > 0 {
            s.push_str(&format!(" ↑{}", self.ahead));
        }
        if self.behind > 0 {
            s.push_str(&format!(" ↓{}", self.behind));
        }
        if n > 0 {
            s.push_str(&format!(" · {n} changed"));
        }
        s
    }
}

fn mark_of(x: char, y: char) -> Mark {
    if x == '?' {
        Mark::Untracked
    } else if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
        Mark::Conflict
    } else if x == 'D' || y == 'D' {
        Mark::Deleted
    } else if x == 'A' {
        Mark::Added
    } else {
        Mark::Modified
    }
}

/// The branch name and how far it is from its upstream, from the `## ` line of `status -b`.
fn parse_branch(line: &str) -> (String, u32, u32) {
    let rest = line.strip_prefix("## ").unwrap_or(line);
    let (name, tail) = match rest.split_once("...") {
        Some((n, t)) => (n, t),
        None => (rest.split_once(" [").map_or(rest, |(n, _)| n), rest.split_once(" [").map_or("", |(_, t)| t)),
    };
    let name = if let Some(n) = name.strip_prefix("No commits yet on ") {
        n.to_owned()
    } else if name.starts_with("HEAD (no branch)") {
        "detached HEAD".to_owned()
    } else {
        name.to_owned()
    };
    let count = |word: &str| tail.split(word).nth(1).and_then(|t| t.split([',', ']']).next()).and_then(|n| n.trim().parse().ok()).unwrap_or(0);
    (name, count("ahead "), count("behind "))
}

/// Read `git status --porcelain=v1 -b -z` output. `prefix` is where the opened folder sits inside the repository
/// (`""` at the top, `"notebooks/"` below it); files outside the folder are left out.
pub fn parse(prefix: &str, output: &str) -> Status {
    let mut st = Status::default();
    let mut entries = output.split('\0');
    while let Some(entry) = entries.next() {
        if let Some(line) = entry.strip_prefix("## ") {
            (st.branch, st.ahead, st.behind) = parse_branch(&format!("## {line}"));
            continue;
        }
        let mut chars = entry.chars();
        let (Some(x), Some(y)) = (chars.next(), chars.next()) else { continue };
        if x == 'R' || x == 'C' {
            // a rename lists the old path next
            entries.next();
        }
        let path = entry.get(3..).unwrap_or("");
        let Some(rel) = path.strip_prefix(prefix) else { continue };
        if rel.is_empty() {
            continue;
        }
        let mark = mark_of(x, y);
        let rel = PathBuf::from(rel);
        for dir in rel.ancestors().skip(1).filter(|a| !a.as_os_str().is_empty()) {
            let worst = st.dirs.entry(dir.to_path_buf()).or_insert(mark);
            *worst = (*worst).max(mark);
        }
        st.files.insert(rel, mark);
    }
    st
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_OPTIONAL_LOCKS", "0").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The status of the repository `folder` is in, or `None` (no `git`, or not a repository).
pub fn status(folder: &Path) -> Option<Status> {
    let prefix = git(folder, &["rev-parse", "--show-prefix"])?.trim().to_owned();
    let out = git(folder, &["-c", "core.quotepath=off", "status", "--porcelain=v1", "-b", "-z", "--untracked-files=all"])?;
    Some(parse(&prefix, &out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_marks_branch_and_folders() {
        let out = "## main...origin/main [ahead 2, behind 1]\0 M notes/a.py\0?? notes/deep/b.py\0A  c.py\0R  new.py\0old.py\0UU conflict.py\0 D gone.py\0 M elsewhere/x.py\0";
        let st = parse("", out);
        assert_eq!((st.branch.as_str(), st.ahead, st.behind), ("main", 2, 1));
        assert_eq!(st.mark(Path::new("notes/a.py"), false), Some(Mark::Modified));
        assert_eq!(st.mark(Path::new("notes/deep/b.py"), false), Some(Mark::Untracked));
        assert_eq!(st.mark(Path::new("c.py"), false), Some(Mark::Added));
        assert_eq!(st.mark(Path::new("new.py"), false), Some(Mark::Modified));
        assert_eq!(st.mark(Path::new("old.py"), false), None, "a rename's old name is not a file of its own");
        assert_eq!(st.mark(Path::new("conflict.py"), false), Some(Mark::Conflict));
        assert_eq!(st.mark(Path::new("gone.py"), false), Some(Mark::Deleted));
        assert_eq!(st.mark(Path::new("notes"), true), Some(Mark::Modified), "a folder shows the worst inside");
        assert_eq!(st.mark(Path::new("notes/deep"), true), Some(Mark::Untracked));
        assert_eq!(st.summary(), "main ↑2 ↓1 · 7 changed");
    }

    #[test]
    fn only_files_below_the_opened_folder_count() {
        let st = parse("notes/", "## dev\0 M notes/a.py\0 M other/b.py\0 M notes/sub/c.py\0");
        assert_eq!(st.files.len(), 2);
        assert_eq!(st.mark(Path::new("a.py"), false), Some(Mark::Modified));
        assert_eq!(st.mark(Path::new("sub"), true), Some(Mark::Modified));
        assert_eq!(st.summary(), "dev · 2 changed");
    }

    #[test]
    fn branch_names_in_every_shape() {
        assert_eq!(parse_branch("## main"), ("main".into(), 0, 0));
        assert_eq!(parse_branch("## No commits yet on trunk"), ("trunk".into(), 0, 0));
        assert_eq!(parse_branch("## HEAD (no branch)"), ("detached HEAD".into(), 0, 0));
        assert_eq!(parse_branch("## feat/x...origin/feat/x [behind 3]"), ("feat/x".into(), 0, 3));
        assert_eq!(parse_branch("## feat...origin/feat [gone]"), ("feat".into(), 0, 0));
    }

    /// Needs the real `git`; passes quietly where it is not installed.
    #[test]
    fn reads_a_real_repository() {
        if git(Path::new("."), &["--version"]).is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("mathcraft-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(std::fs::create_dir_all(dir.join("sub")).is_ok());
        let run = |args: &[&str]| git(&dir, &[&["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"], args].concat()).is_some();
        assert!(run(&["init", "-q", "-b", "main"]));
        assert!(std::fs::write(dir.join("a.py"), "x = 1\n").is_ok());
        assert!(std::fs::write(dir.join("sub/b.py"), "y = 2\n").is_ok());
        assert!(run(&["add", "."]) && run(&["commit", "-q", "-m", "first"]));
        assert!(std::fs::write(dir.join("a.py"), "x = 2\n").is_ok());
        assert!(std::fs::write(dir.join("sub/new.py"), "z = 3\n").is_ok());
        let st = status(&dir).unwrap_or_default();
        assert_eq!(st.branch, "main");
        assert_eq!(st.mark(Path::new("a.py"), false), Some(Mark::Modified));
        assert_eq!(st.mark(Path::new("sub/new.py"), false), Some(Mark::Untracked));
        assert_eq!(st.mark(Path::new("sub"), true), Some(Mark::Untracked));
        // opened one level down, the paths are relative to that folder
        let below = status(&dir.join("sub")).unwrap_or_default();
        assert_eq!(below.mark(Path::new("new.py"), false), Some(Mark::Untracked));
        assert_eq!(below.files.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(status(&std::env::temp_dir().join("mathcraft-not-a-repo-xyz")).is_none());
    }
}
