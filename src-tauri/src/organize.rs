//! Classify Koikatsu cards into [Game]/[Male|Female] (or [Game]/[CardType])
//! folders. Everything here is read-only until `apply` (Task 4).

use crate::card::{read_card, CardError, CardMeta, CardType, DEST_FOLDERS};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
pub enum Collision {
    /// Nothing at the destination name.
    None,
    /// Same name, byte-identical content — the card is already filed.
    AlreadyFiled,
    /// Same name, different content — file the incoming card under a new name.
    Renamed,
}

#[derive(Debug, Serialize)]
pub struct Planned {
    pub from: PathBuf,
    pub to: PathBuf,
    pub collision: Collision,
}

#[derive(Debug, Default, Serialize)]
pub struct ApplyResult {
    pub moved: usize,
    pub already_filed: usize,
    pub renamed: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Unreadable {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct VoiceIssue {
    pub path: PathBuf,
    pub personality: i64,
}

#[derive(Debug, Default, Serialize)]
pub struct Plan {
    pub moves: Vec<Planned>,
    pub skipped: Vec<PathBuf>,
    pub unrecognized: Vec<Unreadable>,
    pub unreadable: Vec<Unreadable>,
    pub voice_incompatible: Vec<VoiceIssue>,
}

/// Where a card belongs, relative to the scan root.
pub fn destination(root: &Path, meta: &CardMeta) -> PathBuf {
    let leaf = match meta.card_type {
        CardType::Character => meta.sex.folder(),
        other => other.folder(),
    };
    root.join(meta.game.folder()).join(leaf)
}

/// THE fix. hamster asked whether the absolute path CONTAINED a game name,
/// so `…/Koikatu_F_20260725003553199_姬野 夜王/card/` matched "Koikatu" and was
/// silently skipped — and that is exactly how Koikatsu names its own card
/// exports, i.e. how card packs are named. Ask instead whether the FIRST path
/// segment relative to the scan root IS one of the destination folders.
/// Anything upstream of the root is irrelevant by construction.
pub fn is_in_dest_folder(root: &Path, file: &Path) -> bool {
    let rel = match file.strip_prefix(root) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let first = match rel.components().next() {
        Some(c) => c.as_os_str().to_string_lossy().to_string(),
        None => return false,
    };
    DEST_FOLDERS.iter().any(|d| d.eq_ignore_ascii_case(&first))
}

fn is_png(p: &Path) -> bool {
    p.extension()
        .map(|x| x.eq_ignore_ascii_case("png"))
        .unwrap_or(false)
}

/// A per-entry failure encountered while walking — distinct from the
/// top-level "this directory itself would not open" case, which is out of
/// scope here: nothing was ever discovered there, so nothing is dropped from
/// a walked set.
enum WalkError {
    /// The `read_dir` iterator yielded an `Err` for some entry inside `dir`
    /// (a per-entry OS enumeration error). No path is available for the
    /// specific entry that failed — only the directory being walked.
    Enumeration { dir: PathBuf },
    /// An entry was enumerated but querying its file type failed: a TOCTOU
    /// deletion mid-walk, or a per-entry permission error. Both are the
    /// realistic case on a network share.
    FileType { path: PathBuf, source: std::io::Error },
}

/// Maps a per-entry walk failure into the `unreadable` bucket, so it is
/// reported rather than silently dropped — the exact failure shape this
/// module exists to eliminate. Kept as a standalone pure function so the
/// mapping is unit-testable even though the underlying OS failures it
/// handles are not portably reproducible in a test (see `mod tests` below).
fn walk_error_to_unreadable(err: WalkError) -> Unreadable {
    match err {
        WalkError::Enumeration { dir } => Unreadable {
            reason: format!("an entry inside {} could not be enumerated", dir.display()),
            path: dir,
        },
        WalkError::FileType { path, source } => {
            Unreadable { reason: format!("could not read file type: {source}"), path }
        }
    }
}

/// Depth-first walk collecting PNG files. Symlinked directories are not
/// followed (DirEntry::file_type does not follow links), so there is no cycle
/// risk. Deliberately not glob-based: card folders routinely contain `[` and
/// `]`, which a glob API would read as a character class.
///
/// Per-entry failures (an enumeration error, or a `file_type()` failure) are
/// pushed into `errors` rather than dropped — every walked file must land in
/// exactly one of the Plan's buckets. `read_dir(dir)` failing outright for
/// an unopenable directory is deliberately not reported: no files were ever
/// discovered there, so nothing is being dropped from a walked set.
fn walk(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>, errors: &mut Vec<Unreadable>) {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    let mut entries = Vec::new();
    for res in rd {
        match res {
            Ok(e) => entries.push(e),
            Err(_) => errors.push(walk_error_to_unreadable(WalkError::Enumeration {
                dir: dir.to_path_buf(),
            })),
        }
    }
    entries.sort_by_key(|e| e.file_name()); // deterministic order for tests
    for e in entries {
        let ft = match e.file_type() {
            Ok(t) => t,
            Err(source) => {
                errors.push(walk_error_to_unreadable(WalkError::FileType {
                    path: e.path(),
                    source,
                }));
                continue;
            }
        };
        if ft.is_dir() {
            if recursive {
                walk(&e.path(), recursive, out, errors);
            }
        } else if ft.is_file() && is_png(&e.path()) {
            out.push(e.path());
        }
    }
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(ma), Ok(mb)) if ma.len() == mb.len() => match (fs::read(a), fs::read(b)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        },
        _ => false,
    }
}

/// Windows treats `A.png` and `a.png` as one file, so a destination name must
/// be matched case-insensitively or a "new" name would silently overwrite.
fn existing_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    let rd = fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Some(e.path());
        }
    }
    None
}

/// `c.png` -> `c (2).png`, `c (3).png`, … skipping names already taken.
fn suffixed(dir: &Path, name: &str) -> PathBuf {
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    for n in 2..10_000 {
        let cand = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        if existing_case_insensitive(dir, &cand).is_none() {
            return dir.join(cand);
        }
    }
    dir.join(name)
}

pub fn apply(plan: &Plan) -> ApplyResult {
    let mut r = ApplyResult::default();
    for m in &plan.moves {
        match m.collision {
            Collision::AlreadyFiled => {
                r.already_filed += 1;
                continue;
            }
            Collision::None | Collision::Renamed => {}
        }
        let dir = match m.to.parent() {
            Some(d) => d,
            None => {
                r.errors.push(format!("{}: destination has no parent", m.to.display()));
                continue;
            }
        };
        if let Err(e) = fs::create_dir_all(dir) {
            r.errors.push(format!("{}: {e}", dir.display()));
            continue;
        }
        // rename() fails across volumes; fall back to copy + remove.
        let moved = fs::rename(&m.from, &m.to).is_ok()
            || (fs::copy(&m.from, &m.to).is_ok() && fs::remove_file(&m.from).is_ok());
        if !moved {
            r.errors.push(format!("{} -> {}: move failed", m.from.display(), m.to.display()));
            continue;
        }
        if m.collision == Collision::Renamed {
            r.renamed += 1;
        } else {
            r.moved += 1;
        }
    }
    r
}

pub fn plan(root: &Path, recursive: bool) -> Plan {
    let mut files = Vec::new();
    let mut p = Plan::default();
    walk(root, recursive, &mut files, &mut p.unreadable);
    for f in files {
        if is_in_dest_folder(root, &f) {
            p.skipped.push(f);
            continue;
        }
        let meta = match read_card(&f) {
            Ok(m) => m,
            Err(e @ CardError::Unrecognized(_)) => {
                p.unrecognized.push(Unreadable { path: f, reason: e.reason() });
                continue;
            }
            Err(e) => {
                p.unreadable.push(Unreadable { path: f, reason: e.reason() });
                continue;
            }
        };
        let name = match f.file_name() {
            Some(n) => n.to_os_string(),
            None => continue,
        };
        let dir = destination(root, &meta);
        let name_s = name.to_string_lossy().to_string();
        let (to, collision) = match existing_case_insensitive(&dir, &name_s) {
            None => (dir.join(&name_s), Collision::None),
            Some(ex) if same_bytes(&f, &ex) => (ex, Collision::AlreadyFiled),
            Some(_) => (suffixed(&dir, &name_s), Collision::Renamed),
        };
        p.moves.push(Planned { from: f, to, collision });
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `read_dir`'s iterator yielding an `Err` item mid-enumeration (a
    /// per-entry OS error, realistic on a network share) has no path for the
    /// specific entry — only the directory being walked. It must still be
    /// reported, not dropped.
    #[test]
    fn an_enumeration_failure_is_reported_against_the_containing_directory() {
        let dir = PathBuf::from(r"Z:\cardpacks\broken");
        let u = walk_error_to_unreadable(WalkError::Enumeration { dir: dir.clone() });
        assert_eq!(u.path, dir);
        assert!(u.reason.contains("could not be enumerated"), "{}", u.reason);
    }

    /// An entry was enumerated but `file_type()` failed on it (TOCTOU
    /// deletion mid-walk, or a per-entry permission error). It must be
    /// reported against the entry itself, not dropped.
    #[test]
    fn a_file_type_failure_is_reported_against_the_entry_itself() {
        let path = PathBuf::from(r"Z:\cardpacks\broken\c.png");
        let source = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let u = walk_error_to_unreadable(WalkError::FileType { path: path.clone(), source });
        assert_eq!(u.path, path);
        assert!(u.reason.contains("file type"), "{}", u.reason);
        assert!(u.reason.contains("access denied"), "{}", u.reason);
    }
}
