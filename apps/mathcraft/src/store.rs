//! What MathCraft keeps for notebooks beside `session.json`, in the config folder: the last results of each notebook
//! (`outputs/<key>.json`) and the text of untitled notebooks that have been typed into (`drafts/<name>.py`).
//!
//! Results are kept here rather than next to the notebook so a `.py` file stays plain and its diffs clean, and no
//! stray files appear in a project. A notebook is found again by its path, so moving or renaming it forgets them.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Results of notebooks that were not opened for this long are deleted at startup.
const KEEP: Duration = Duration::from_secs(60 * 24 * 3600);

/// A name for a notebook's results: a hash of its path, or the draft's name.
pub fn key_for(path: &Path) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in path.to_string_lossy().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn outputs_file(dir: &Path, key: &str) -> PathBuf {
    dir.join("outputs").join(format!("{key}.json"))
}

fn draft_file(dir: &Path, name: &str) -> PathBuf {
    dir.join("drafts").join(format!("{name}.py"))
}

/// Draft names are used as file names, so only the ones this module makes are accepted.
pub fn valid_draft_name(name: &str) -> bool {
    name.strip_prefix("draft-").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

pub fn read_outputs(dir: &Path, key: &str) -> Option<String> {
    std::fs::read_to_string(outputs_file(dir, key)).ok()
}

pub fn write_outputs(dir: &Path, key: &str, json: &str) -> std::io::Result<()> {
    write_atomic(&outputs_file(dir, key), json)
}

pub fn forget_outputs(dir: &Path, key: &str) {
    let _ = std::fs::remove_file(outputs_file(dir, key));
}

pub fn read_draft(dir: &Path, name: &str) -> Option<String> {
    if !valid_draft_name(name) {
        return None;
    }
    std::fs::read_to_string(draft_file(dir, name)).ok()
}

pub fn write_draft(dir: &Path, name: &str, text: &str) -> std::io::Result<()> {
    if !valid_draft_name(name) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad draft name"));
    }
    write_atomic(&draft_file(dir, name), text)
}

/// Delete a draft and its results (it was saved to a file, or closed).
pub fn forget_draft(dir: &Path, name: &str) {
    if valid_draft_name(name) {
        let _ = std::fs::remove_file(draft_file(dir, name));
        forget_outputs(dir, name);
    }
}

/// A draft name not used by a file in `dir` or by one of `taken`.
pub fn new_draft_name(dir: Option<&Path>, taken: &[String]) -> String {
    (1..)
        .map(|n| format!("draft-{n}"))
        .find(|name| !taken.contains(name) && dir.is_none_or(|d| !draft_file(d, name).exists() && !outputs_file(d, name).exists()))
        .unwrap_or_else(|| "draft-0".to_owned())
}

/// Delete saved results that have not been touched for a long time, and drafts nothing refers to.
pub fn prune(dir: &Path, live_drafts: &[String]) {
    let old =
        |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|age| age > KEEP);
    if let Ok(entries) = std::fs::read_dir(dir.join("outputs")) {
        for e in entries.flatten() {
            let stem = e.path().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let orphan_draft = valid_draft_name(&stem) && !live_drafts.contains(&stem);
            if orphan_draft || old(&e.path()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("drafts")) {
        for e in entries.flatten() {
            let stem = e.path().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if !live_drafts.contains(&stem) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Write beside the target, then rename, so a crash never leaves half a file.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mathcraft-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn results_and_drafts_round_trip_and_are_forgotten_together() {
        let dir = scratch("round");
        let key = key_for(Path::new("/a/b.py"));
        assert_ne!(key, key_for(Path::new("/a/c.py")));
        assert_eq!(read_outputs(&dir, &key), None);
        write_outputs(&dir, &key, "{}").ok();
        assert_eq!(read_outputs(&dir, &key).as_deref(), Some("{}"));
        write_draft(&dir, "draft-1", "# %%\n1\n").ok();
        write_outputs(&dir, "draft-1", "{}").ok();
        assert_eq!(read_draft(&dir, "draft-1").as_deref(), Some("# %%\n1\n"));
        forget_draft(&dir, "draft-1");
        assert_eq!((read_draft(&dir, "draft-1"), read_outputs(&dir, "draft-1")), (None, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn draft_names_cannot_escape_the_folder() {
        assert!(valid_draft_name("draft-12") && !valid_draft_name("draft-") && !valid_draft_name("../x") && !valid_draft_name("draft-1/../../x"));
        let dir = scratch("names");
        assert!(write_draft(&dir, "../evil", "x").is_err());
        assert_eq!(read_draft(&dir, "../evil"), None);
        let taken = vec!["draft-1".to_owned()];
        assert_eq!(new_draft_name(None, &taken), "draft-2");
        write_draft(&dir, "draft-2", "").ok();
        assert_eq!(new_draft_name(Some(&dir), &taken), "draft-3");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pruning_drops_drafts_nothing_refers_to() {
        let dir = scratch("prune");
        write_draft(&dir, "draft-1", "a").ok();
        write_draft(&dir, "draft-2", "b").ok();
        write_outputs(&dir, "draft-2", "{}").ok();
        prune(&dir, &["draft-1".to_owned()]);
        assert!(read_draft(&dir, "draft-1").is_some());
        assert_eq!((read_draft(&dir, "draft-2"), read_outputs(&dir, "draft-2")), (None, None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
