//! Directory watching for the image auto-preview feature.
//!
//! A local media-generation pipeline (SDXL / ComfyUI / Wan) drops finished PNG /
//! JPG / WebP files into an output directory. When runnir is armed on that
//! directory it polls it on the existing periodic tick and, when a genuinely new
//! and fully-written file appears, previews it inline in the focused pane through
//! the same kitty-graphics placement path used for `icat`.
//!
//! The detection is deliberately a pure state machine over directory listings
//! (`WatchState` + `step`), so it can be unit-tested without touching the real
//! filesystem: `list_dir` is the only part that reads disk, and it just produces
//! the `FileStat` slice the state machine consumes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A file's identity for change detection: its path plus the two cheap signals
/// that tell a still-being-written file from a finished one — its size and its
/// modification time (as nanoseconds since the epoch, 0 if unknown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: u128,
}

/// The debounce state for one watched directory.
///
/// `seen` holds every file that must not fire again *at its recorded size and
/// mtime*: the snapshot taken when the watch was armed (so pre-existing files never
/// flood the pane) plus every file already previewed. Keying on the stat, not just
/// the path, means a seen file that is later rewritten (or was still being written
/// when the watch was armed) re-enters the debounce and fires once it settles —
/// only a file that is byte-for-byte where we left it stays suppressed. `pending`
/// holds candidate files observed once but not yet confirmed stable, so a file
/// still being written waits until its size and mtime stop changing before it is
/// previewed.
#[derive(Debug, Default)]
pub struct WatchState {
    seen: HashMap<PathBuf, (u64, u128)>,
    pending: HashMap<PathBuf, (u64, u128)>,
}

impl WatchState {
    /// Arms the watch on an initial listing: every file present now is recorded as
    /// already seen, so only files created or modified after this fire.
    pub fn armed(listing: &[FileStat]) -> Self {
        Self {
            seen: listing.iter().map(|f| (f.path.clone(), (f.size, f.mtime))).collect(),
            pending: HashMap::new(),
        }
    }

    /// Advances the state machine by one poll and returns the files that became
    /// ready to preview this tick, oldest first (so the caller can take the last as
    /// the newest). A new file is reported only on the poll where its size and mtime
    /// match the previous poll — one tick of stability, which debounces a file that
    /// is still being written.
    pub fn step(&mut self, listing: &[FileStat]) -> Vec<PathBuf> {
        let current: HashSet<&PathBuf> = listing.iter().map(|f| &f.path).collect();
        // A candidate that vanished before stabilising is forgotten, so a later file
        // reusing its name starts its debounce afresh.
        self.pending.retain(|p, _| current.contains(p));

        let mut ready: Vec<(PathBuf, u128)> = Vec::new();
        for f in listing {
            // Suppressed only while the file is exactly as recorded; a rewrite (new
            // size or mtime) falls through and debounces like any new file.
            if self.seen.get(&f.path) == Some(&(f.size, f.mtime)) {
                continue;
            }
            match self.pending.get(&f.path) {
                // Unchanged since the previous sighting: fully written, preview it.
                Some(&(size, mtime)) if size == f.size && mtime == f.mtime => {
                    self.seen.insert(f.path.clone(), (f.size, f.mtime));
                    self.pending.remove(&f.path);
                    ready.push((f.path.clone(), f.mtime));
                }
                // First sighting, or still growing: record and wait one more tick.
                _ => {
                    self.pending.insert(f.path.clone(), (f.size, f.mtime));
                }
            }
        }
        ready.sort_by_key(|(_, m)| *m);
        ready.into_iter().map(|(p, _)| p).collect()
    }
}

/// Whether a path's extension is in the filter. An empty filter matches every
/// file, so `extensions = []` in the config means "any file dropped here".
pub fn matches_ext(path: &Path, exts: &[String]) -> bool {
    if exts.is_empty() {
        return true;
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => exts.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(ext)),
        None => false,
    }
}

/// Lists the regular files in `dir` matching the extension filter, with their size
/// and mtime. An unreadable directory yields an empty listing rather than an error:
/// a watch pointed at a not-yet-created output dir simply finds nothing until it
/// appears. Never blocks beyond a single `read_dir`.
pub fn list_dir(dir: &Path, exts: &[String]) -> Vec<FileStat> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !matches_ext(&path, exts) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        out.push(FileStat { path, size: meta.len(), mtime });
    }
    out
}

/// Expands a bare `~` or a leading `~/` to `$HOME`, leaving every other path
/// (including `~user`, which would need a passwd lookup) untouched. The watched
/// directory is user-typed (config or the runtime prompt), so a home-tilde is the
/// one shorthand worth honouring.
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    } else if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(name: &str, size: u64, mtime: u128) -> FileStat {
        FileStat { path: PathBuf::from(name), size, mtime }
    }

    #[test]
    fn preexisting_files_never_fire() {
        // The snapshot taken on arm must suppress every file already there, or the
        // first poll would flood the pane with the whole back-catalogue.
        let listing = vec![stat("a.png", 10, 1), stat("b.png", 20, 2)];
        let mut state = WatchState::armed(&listing);
        assert!(state.step(&listing).is_empty(), "old files must not fire");
    }

    #[test]
    fn a_new_file_fires_only_after_one_stable_tick() {
        // Debounce: a file must be seen unchanged across two polls before it fires,
        // so a half-written file is never previewed mid-write.
        let mut state = WatchState::armed(&[stat("a.png", 10, 1)]);
        let listing = vec![stat("a.png", 10, 1), stat("b.png", 100, 5)];
        assert!(state.step(&listing).is_empty(), "first sighting must wait");
        assert_eq!(state.step(&listing), vec![PathBuf::from("b.png")], "stable → fires");
        assert!(state.step(&listing).is_empty(), "a fired file never fires again");
    }

    #[test]
    fn a_growing_file_waits_until_its_size_settles() {
        // A file whose size keeps changing is still being written; it must not fire
        // until it holds steady.
        let mut state = WatchState::armed(&[]);
        assert!(state.step(&[stat("big.png", 5, 1)]).is_empty(), "first sighting");
        assert!(state.step(&[stat("big.png", 500, 2)]).is_empty(), "grew: not yet");
        assert!(state.step(&[stat("big.png", 900, 3)]).is_empty(), "grew again: not yet");
        assert_eq!(
            state.step(&[stat("big.png", 900, 3)]),
            vec![PathBuf::from("big.png")],
            "size and mtime finally settled → fires"
        );
    }

    #[test]
    fn the_newest_of_several_is_reported_last() {
        // step returns oldest-first so the caller can preview the newest by taking
        // the last entry.
        let mut state = WatchState::armed(&[]);
        let batch = vec![stat("old.png", 1, 10), stat("new.png", 1, 30), stat("mid.png", 1, 20)];
        assert!(state.step(&batch).is_empty(), "first sighting of all");
        let ready = state.step(&batch);
        assert_eq!(ready.last(), Some(&PathBuf::from("new.png")), "newest by mtime is last");
        assert_eq!(ready.len(), 3);
    }

    #[test]
    fn a_candidate_that_vanishes_is_forgotten() {
        // A temp file that appears and is renamed away must not linger in pending and
        // fire later if its name is reused.
        let mut state = WatchState::armed(&[]);
        assert!(state.step(&[stat("tmp.png", 5, 1)]).is_empty(), "seen once");
        assert!(state.step(&[]).is_empty(), "gone before stabilising");
        // Same name reappears, different content: it debounces from scratch.
        assert!(state.step(&[stat("tmp.png", 9, 2)]).is_empty(), "fresh debounce");
        assert_eq!(state.step(&[stat("tmp.png", 9, 2)]), vec![PathBuf::from("tmp.png")]);
    }

    #[test]
    fn a_preexisting_file_still_being_written_at_arm_fires_when_it_settles() {
        // Arming while the pipeline is mid-write must not swallow that render: the
        // half-written file is in the arm snapshot, but once its size/mtime move on
        // and then settle, it is a modified file and fires. Only a file byte-for-byte
        // where the snapshot left it stays suppressed.
        let mut state = WatchState::armed(&[stat("render.png", 100, 1)]);
        // The writer finishes: the stat no longer matches the snapshot.
        assert!(state.step(&[stat("render.png", 5000, 9)]).is_empty(), "changed: debounce");
        assert_eq!(
            state.step(&[stat("render.png", 5000, 9)]),
            vec![PathBuf::from("render.png")],
            "a pre-existing file finished after arm fires once stable"
        );
        assert!(state.step(&[stat("render.png", 5000, 9)]).is_empty(), "then stays quiet");
    }

    #[test]
    fn a_bare_tilde_expands_to_home() {
        // "~" with no slash is the natural thing to type at the watch-dir prompt; it
        // must mean $HOME, not a literal ./~ directory. ~user stays untouched.
        let home = std::env::var_os("HOME").expect("HOME set in tests");
        assert_eq!(expand_tilde("~"), PathBuf::from(&home));
        assert_eq!(expand_tilde("~/x"), PathBuf::from(&home).join("x"));
        assert_eq!(expand_tilde("~other"), PathBuf::from("~other"));
        assert_eq!(expand_tilde("/abs"), PathBuf::from("/abs"));
    }

    #[test]
    fn extension_filter_is_case_insensitive_and_dot_tolerant() {
        let exts = vec!["png".to_string(), ".JPG".to_string()];
        assert!(matches_ext(Path::new("x.png"), &exts));
        assert!(matches_ext(Path::new("x.PNG"), &exts));
        assert!(matches_ext(Path::new("x.jpg"), &exts));
        assert!(!matches_ext(Path::new("x.gif"), &exts));
        assert!(!matches_ext(Path::new("noext"), &exts));
        // An empty filter matches anything.
        assert!(matches_ext(Path::new("x.gif"), &[]));
    }
}
