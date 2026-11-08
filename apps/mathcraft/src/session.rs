//! What to bring back on the next launch: the files open in tabs, which tab was active, the opened folder, whether
//! the sidebar was showing, and the recently opened files. Stored as `session.json` beside `settings.json`; a
//! missing or malformed file just means a fresh start.

use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Most recent files remembered.
pub const MAX_RECENT: usize = 12;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    /// Saved files open in tabs, in tab order (untitled notebooks are not kept).
    pub files: Vec<PathBuf>,
    /// Index into `files` of the tab that was showing.
    pub active: usize,
    pub folder: Option<PathBuf>,
    pub sidebar: bool,
    /// Newest first.
    pub recent: Vec<PathBuf>,
}

impl Session {
    pub fn parse(text: &str) -> Session {
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(text) else { return Session::default() };
        let paths = |key: &str| -> Vec<PathBuf> {
            m.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(PathBuf::from).collect()).unwrap_or_default()
        };
        Session {
            files: paths("files"),
            active: m.get("active").and_then(Value::as_u64).map_or(0, |n| n as usize),
            folder: m.get("folder").and_then(Value::as_str).map(PathBuf::from),
            sidebar: m.get("sidebar").and_then(Value::as_bool).unwrap_or(false),
            recent: paths("recent").into_iter().take(MAX_RECENT).collect(),
        }
    }

    pub fn to_json(&self) -> String {
        let list = |v: &[PathBuf]| v.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let mut m = Map::new();
        m.insert("files".into(), json!(list(&self.files)));
        m.insert("active".into(), json!(self.active));
        if let Some(f) = &self.folder {
            m.insert("folder".into(), json!(f.to_string_lossy()));
        }
        m.insert("sidebar".into(), json!(self.sidebar));
        m.insert("recent".into(), json!(list(&self.recent)));
        serde_json::to_string_pretty(&Value::Object(m)).unwrap_or_default() + "\n"
    }

    pub fn load(dir: &Path) -> Session {
        std::fs::read_to_string(dir.join("session.json")).map(|t| Session::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("session.json"), self.to_json())
    }

    /// Put `path` first in the recent list (once), and drop the oldest beyond the limit.
    pub fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_ignores_junk() {
        let s =
            Session { files: vec!["/a/x.py".into(), "/a/y.py".into()], active: 1, folder: Some("/a".into()), sidebar: true, recent: vec!["/a/y.py".into()] };
        assert_eq!(Session::parse(&s.to_json()), s);
        assert_eq!(Session::parse("nope"), Session::default());
        assert_eq!(Session::parse(r#"{"files": [1, "b.py"], "active": "x"}"#).files, vec![PathBuf::from("b.py")]);
    }

    #[test]
    fn recent_files_are_newest_first_without_repeats() {
        let mut s = Session::default();
        for n in 0..20 {
            s.remember(Path::new(&format!("/f{n}.py")));
        }
        s.remember(Path::new("/f15.py"));
        assert_eq!(s.recent.len(), MAX_RECENT);
        assert_eq!(s.recent.first(), Some(&PathBuf::from("/f15.py")));
        assert_eq!(s.recent.iter().filter(|p| p.ends_with("f15.py")).count(), 1);
    }
}
