//! Classify Koikatsu cards into [Game]/[Male|Female] (or [Game]/[CardType])
//! folders. Everything here is read-only until `apply` (Task 4).

use crate::card::{read_card, CardError, CardMeta, CardType, DEST_FOLDERS};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct Planned {
    pub from: PathBuf,
    pub to: PathBuf,
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

/// Depth-first walk collecting PNG files. Symlinked directories are not
/// followed (DirEntry::file_type does not follow links), so there is no cycle
/// risk. Deliberately not glob-based: card folders routinely contain `[` and
/// `]`, which a glob API would read as a character class.
fn walk(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name()); // deterministic order for tests
    for e in entries {
        let ft = match e.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            if recursive {
                walk(&e.path(), recursive, out);
            }
        } else if ft.is_file() && is_png(&e.path()) {
            out.push(e.path());
        }
    }
}

pub fn plan(root: &Path, recursive: bool) -> Plan {
    let mut files = Vec::new();
    walk(root, recursive, &mut files);
    let mut p = Plan::default();
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
        let to = destination(root, &meta).join(name);
        p.moves.push(Planned { from: f, to });
    }
    p
}
