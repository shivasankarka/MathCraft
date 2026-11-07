//! A folder of notebooks: the file tree shown in the sidebar. Pure file-system logic, no UI.

use std::path::{Path, PathBuf};

/// Deepest folder level listed (the opened folder is level 0).
const MAX_DEPTH: usize = 4;
/// Most entries read from one folder, so a huge directory cannot stall the UI.
const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Dir { path: PathBuf, name: String, children: Vec<Node> },
    File { path: PathBuf, name: String },
}

impl Node {
    pub fn name(&self) -> &str {
        match self {
            Node::Dir { name, .. } | Node::File { name, .. } => name,
        }
    }
}

fn is_notebook(name: &str) -> bool {
    name.ends_with(".py")
}

fn is_hidden_dir(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules" | "__pycache__" | "venv")
}

/// The notebooks under `dir`: folders first, then files, each alphabetical (case-insensitive). Folders without
/// notebooks are left out. Unreadable folders are skipped.
pub fn scan(dir: &Path) -> Vec<Node> {
    scan_level(dir, 0)
}

fn scan_level(dir: &Path, depth: usize) -> Vec<Node> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    for entry in read.flatten().take(MAX_ENTRIES) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if depth < MAX_DEPTH && !is_hidden_dir(&name) {
                let children = scan_level(&path, depth + 1);
                if !children.is_empty() {
                    dirs.push(Node::Dir { path, name, children });
                }
            }
        } else if kind.is_file() && is_notebook(&name) && !name.starts_with('.') {
            files.push(Node::File { path, name });
        }
    }
    let key = |n: &Node| n.name().to_lowercase();
    dirs.sort_by_key(key);
    files.sort_by_key(key);
    dirs.extend(files);
    dirs
}

/// A file name in `dir` that does not exist yet: `untitled.py`, `untitled-2.py`, ...
pub fn unused_name(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.py"));
    if !first.exists() {
        return first;
    }
    (2..10_000).map(|n| dir.join(format!("{stem}-{n}.py"))).find(|p| !p.exists()).unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mathcraft-ws-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_notebooks_folders_first_and_skips_noise() {
        let dir = temp("scan");
        for f in ["b.py", "A.py", "notes.txt", ".hidden.py", "sub/inner.py", "empty/readme.md", "target/x.py", ".git/y.py"] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "# %%\n").unwrap();
        }
        let tree = scan(&dir);
        let names: Vec<&str> = tree.iter().map(Node::name).collect();
        assert_eq!(names, vec!["sub", "A.py", "b.py"]);
        let Some(Node::Dir { children, .. }) = tree.first() else { panic!("expected a folder first") };
        assert_eq!(children.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn picks_unused_file_names() {
        let dir = temp("names");
        assert_eq!(unused_name(&dir, "untitled").file_name().unwrap(), "untitled.py");
        std::fs::write(dir.join("untitled.py"), "").unwrap();
        assert_eq!(unused_name(&dir, "untitled").file_name().unwrap(), "untitled-2.py");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_is_just_empty() {
        assert!(scan(Path::new("/definitely/not/here")).is_empty());
    }
}
