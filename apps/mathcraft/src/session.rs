//! What to bring back on the next launch: the tabs (saved files and untitled drafts) and which one was active, where
//! each was scrolled to and where its cursor was, the opened folder, whether the sidebar was showing, and the
//! recently opened files. Stored as `session.json` beside `settings.json`; a missing or malformed file just means a
//! fresh start.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Most recent files remembered.
pub const MAX_RECENT: usize = 12;

/// A tab that can be brought back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tab {
    File(PathBuf),
    /// An untitled notebook, kept under this name (see `store`).
    Draft(String),
}

impl Tab {
    /// How the tab is written in `session.json`: a path, or `draft:<name>` (paths are absolute, so never look like it).
    pub fn key(&self) -> String {
        match self {
            Tab::File(p) => p.to_string_lossy().into_owned(),
            Tab::Draft(n) => format!("draft:{n}"),
        }
    }

    pub fn from_key(key: &str) -> Tab {
        match key.strip_prefix("draft:") {
            Some(name) => Tab::Draft(name.to_owned()),
            None => Tab::File(PathBuf::from(key)),
        }
    }
}

/// Where a tab was left: how far it was scrolled, and the cell and the place in it that the cursor was at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Memo {
    pub scroll: u32,
    /// The cell the cursor was in and its character offset there; `None` when no cell had the keyboard.
    pub cursor: Option<(usize, usize)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    /// Open tabs, in order.
    pub tabs: Vec<Tab>,
    /// Index into `tabs` of the tab that was showing.
    pub active: usize,
    pub folder: Option<PathBuf>,
    pub sidebar: bool,
    /// Newest first.
    pub recent: Vec<PathBuf>,
    /// By `Tab::key`.
    pub views: BTreeMap<String, Memo>,
}

impl Session {
    pub fn parse(text: &str) -> Session {
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(text) else { return Session::default() };
        let strings = |key: &str| -> Vec<String> {
            m.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
        };
        // older versions wrote `files`, saved files only
        let tabs = if m.contains_key("tabs") { strings("tabs") } else { strings("files") };
        let num = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64);
        let views = m
            .get("views")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(k, v)| {
                        let memo = Memo {
                            scroll: num(v, "scroll").and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                            cursor: num(v, "cell").zip(num(v, "caret")).map(|(c, k)| (c as usize, k as usize)),
                        };
                        (k.clone(), memo)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Session {
            tabs: tabs.iter().map(|k| Tab::from_key(k)).collect(),
            active: m.get("active").and_then(Value::as_u64).map_or(0, |n| n as usize),
            folder: m.get("folder").and_then(Value::as_str).map(PathBuf::from),
            sidebar: m.get("sidebar").and_then(Value::as_bool).unwrap_or(false),
            recent: strings("recent").into_iter().map(PathBuf::from).take(MAX_RECENT).collect(),
            views,
        }
    }

    pub fn to_json(&self) -> String {
        let list = |v: &[PathBuf]| v.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let mut m = Map::new();
        m.insert("tabs".into(), json!(self.tabs.iter().map(Tab::key).collect::<Vec<_>>()));
        m.insert("active".into(), json!(self.active));
        if let Some(f) = &self.folder {
            m.insert("folder".into(), json!(f.to_string_lossy()));
        }
        m.insert("sidebar".into(), json!(self.sidebar));
        m.insert("recent".into(), json!(list(&self.recent)));
        let views: Map<String, Value> = self
            .views
            .iter()
            .map(|(k, v)| {
                let mut o = Map::new();
                o.insert("scroll".into(), json!(v.scroll));
                if let Some((cell, caret)) = v.cursor {
                    o.insert("cell".into(), json!(cell));
                    o.insert("caret".into(), json!(caret));
                }
                (k.clone(), Value::Object(o))
            })
            .collect();
        m.insert("views".into(), Value::Object(views));
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
        let mut s = Session {
            tabs: vec![Tab::File("/a/x.py".into()), Tab::Draft("draft-2".into()), Tab::File("/a/y.py".into())],
            active: 1,
            folder: Some("/a".into()),
            sidebar: true,
            recent: vec!["/a/y.py".into()],
            views: BTreeMap::new(),
        };
        s.views.insert("/a/x.py".into(), Memo { scroll: 420, cursor: Some((3, 17)) });
        s.views.insert("draft:draft-2".into(), Memo::default());
        assert_eq!(Session::parse(&s.to_json()), s);
        assert_eq!(Session::parse("nope"), Session::default());
        assert_eq!(Session::parse(r#"{"tabs": [1, "b.py"], "active": "x", "views": {"b.py": 5}}"#).tabs, vec![Tab::File("b.py".into())]);
    }

    #[test]
    fn sessions_from_older_versions_still_open() {
        let s = Session::parse(r#"{"files": ["/a/x.py", "/a/y.py"], "active": 1, "sidebar": false, "recent": []}"#);
        assert_eq!(s.tabs, vec![Tab::File("/a/x.py".into()), Tab::File("/a/y.py".into())]);
        assert_eq!(s.active, 1);
        assert!(s.views.is_empty());
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
