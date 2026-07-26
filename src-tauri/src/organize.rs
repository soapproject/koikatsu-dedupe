//! Classify Koikatsu cards into [Game]/[Male|Female] (or [Game]/[CardType])
//! folders. Everything here is read-only until `apply` (Task 4).

use crate::card::{read_card, CardError, CardMeta, CardType, DEST_FOLDERS};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
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
    /// Same name, different content, and every `name (N)` slot up to the
    /// search cap is taken (by disk or by another file in this same plan).
    /// There is no safe destination to hand back, so nothing is planned to
    /// move — `apply` reports this as an error instead of guessing.
    Unresolvable,
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

/// Which personality ids the target install can actually voice.
///
/// Derived from the install, never hardcoded: a personality mod changes the
/// answer. Scope note — only the base `abdata` tree is read. Sideloader
/// zipmods could in principle add `sound/data/pcm/c<N>`, but reading them
/// needs a zip dependency this project does not carry, and a sweep of a real
/// 20,502-mod install found none doing so (the set was exactly the base
/// game's contiguous 0-38). `source` records what was actually scanned so a
/// report never overstates its own coverage.
///
/// `ok` distinguishes "the scan ran and the install genuinely supports these
/// (possibly zero) personalities" from "the scan could not run at all"
/// (nonexistent or unreadable `pcm` directory — e.g. a mistyped game root).
/// Those two cases would otherwise both present as an empty `ids`, and a
/// failed scan treated as "supports nothing" would flag every Sunshine card
/// with a personality — exactly the guessed supported set this module is
/// built to avoid. When `ok` is `false`, callers must make no voice claim at
/// all, the same as if no `VoiceSupport` had been supplied.
#[derive(Debug, Clone)]
pub struct VoiceSupport {
    pub ids: BTreeSet<i64>,
    pub source: String,
    pub ok: bool,
}

/// Reads the personality ids a game install can voice, from its
/// `abdata/sound/data/pcm/c<N>` folders. Sideloader-added zipmods are not
/// scanned (see `VoiceSupport` doc) — `source` says so explicitly. If the
/// `pcm` directory itself cannot be read (missing or inaccessible — the
/// realistic shape of a mistyped game root), `ok` is `false` and `source`
/// says the scan failed rather than quietly reporting zero personality ids
/// as though a real, empty install had been read.
pub fn voice_support(game_root: &Path) -> VoiceSupport {
    let dir = game_root.join("abdata").join("sound").join("data").join("pcm");
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => {
            return VoiceSupport {
                ids: BTreeSet::new(),
                source: format!(
                    "{}: scan failed ({e}) — no voice claim can be made",
                    dir.display()
                ),
                ok: false,
            };
        }
    };
    let mut ids = BTreeSet::new();
    for e in rd.flatten() {
        // Only directories are personalities. A stray file that happens to
        // be named like one (`c5`) must not be counted as support for it.
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        // `c00`..`c38` are personalities; `c-1`, `c-100` etc. are special voices.
        if let Some(rest) = name.strip_prefix('c') {
            if let Ok(n) = rest.parse::<i64>() {
                if n >= 0 {
                    ids.insert(n);
                }
            }
        }
    }
    VoiceSupport {
        source: format!(
            "{} ({} personality ids; base abdata only — Sideloader-added personalities are not scanned)",
            dir.display(),
            ids.len()
        ),
        ids,
        ok: true,
    }
}

/// Whether `meta`'s personality is one `voice` confirms the install can
/// actually speak. `None` means no flag: either `meta` isn't a Sunshine
/// card (KK is always the conversion target, never the thing being
/// checked), no `VoiceSupport` was supplied, the scan behind it failed
/// (`ok == false` — treated exactly like no `VoiceSupport` at all, per its
/// doc comment), or the card has no personality to check. `Some(pid)` means
/// `pid` is not in the install's supported set and should be flagged.
fn voice_issue(meta: &CardMeta, voice: Option<&VoiceSupport>) -> Option<i64> {
    if meta.game != crate::card::Game::KoikatsuSunshine {
        return None;
    }
    let v = voice?;
    if !v.ok {
        return None;
    }
    let pid = meta.personality?;
    if v.ids.contains(&pid) {
        None
    } else {
        Some(pid)
    }
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

/// A per-entry failure encountered while scanning a destination directory for
/// a name collision — the same failure class `WalkError::Enumeration` guards
/// against in `walk`, just triggered by a `read_dir` used for a different
/// purpose. No path is available for the specific entry that failed, only
/// the directory being scanned.
struct CollisionScanError {
    dir: PathBuf,
    source: std::io::Error,
}

/// Maps a collision-scan enumeration failure to an error string. Kept as a
/// standalone pure function, mirroring `walk_error_to_unreadable`, so the
/// mapping is unit-testable even though the underlying OS failure it handles
/// is not portably reproducible in a test (see `mod tests` below). The
/// caller MUST treat this as "collision status unknown", never as "no
/// collision" — silently assuming no collision here is exactly the class of
/// bug `apply` would then turn into a silent overwrite.
fn collision_scan_error_to_reason(err: CollisionScanError) -> String {
    format!(
        "{}: could not enumerate an entry while checking for a name collision: {}",
        err.dir.display(),
        err.source
    )
}

/// Windows treats `A.png` and `a.png` as one file, so a destination name must
/// be matched case-insensitively or a "new" name would silently overwrite.
/// `Ok(None)` means "nothing there" — including the destination directory not
/// existing YET (`NotFound`), where nothing has been filed, so there is
/// nothing to collide with. Every OTHER `read_dir` failure (a dropped network
/// share, an ACL that denies list while permitting write) leaves the answer
/// unknowable and must be an `Err`: treating it as "no collision" is what
/// turns `apply`'s `fs::rename` into a silent overwrite of a card that is
/// there but was never seen. Same rule for a per-entry enumeration failure.
fn existing_case_insensitive(dir: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "{}: the destination directory could not be listed ({e}), so it is unknown whether a card named {name:?} is already there",
                dir.display()
            ));
        }
    };
    // Windows' own folding table is not `to_lowercase`, but over-matching only
    // ever moves a case from "no collision" to "collision detected" — the safe
    // direction. `eq_ignore_ascii_case` under-matches instead: `Ç.png` and
    // `ç.png` are one file on Windows, and calling them distinct hands `apply`
    // two destinations that are really one.
    let want = name.to_lowercase();
    for res in rd {
        match res {
            Ok(e) => {
                if e.file_name().to_string_lossy().to_lowercase() == want {
                    return Ok(Some(e.path()));
                }
            }
            Err(source) => {
                return Err(collision_scan_error_to_reason(CollisionScanError {
                    dir: dir.to_path_buf(),
                    source,
                }));
            }
        }
    }
    Ok(None)
}

/// A destination name already spoken for during the current `plan()` pass,
/// before anything has actually moved. `source` is the original (still
/// unmoved) file that claimed it, so a later collision against the same name
/// can be resolved by content without touching a destination file that
/// doesn't exist yet.
struct Claim {
    dest: PathBuf,
    source: PathBuf,
}

/// Folds a destination name to the key two claims collide on. Must agree with
/// `existing_case_insensitive`'s comparison, and must fold NON-ASCII case too:
/// `Ç.png` and `ç.png` are one file on Windows, so two pending sources named
/// that way must be resolved against each other rather than each planning a
/// move to "its own" name that `apply` then collapses into one overwrite.
fn claim_key(name: &str) -> String {
    name.to_lowercase()
}

/// `c.png` -> `c (2).png`, `c (3).png`, … skipping names already taken on
/// disk OR already claimed earlier in this same plan. `Ok(None)` means the
/// search space (2..10_000) is exhausted — the caller must not fall back to
/// the colliding name, since that is precisely the name this function exists
/// to avoid handing back.
fn suffixed(
    dir: &Path,
    name: &str,
    claims: &HashMap<String, Claim>,
) -> Result<Option<PathBuf>, String> {
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    for n in 2..10_000 {
        let cand = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        if claims.contains_key(&claim_key(&cand)) {
            continue;
        }
        if existing_case_insensitive(dir, &cand)?.is_none() {
            return Ok(Some(dir.join(cand)));
        }
    }
    Ok(None)
}

/// Whether this exact content is ALREADY claimed somewhere in `dir` — under
/// any name, including a suffixed one. Without this, three same-named cards
/// bound for one directory file two identical copies: the first takes `c.png`,
/// the second differs and takes `c (2).png`, and the third — byte-identical to
/// the second — is only ever compared against the FIRST, so it differs too and
/// takes `c (3).png`. That manufactures exactly the duplicate the collision
/// rule exists to prevent, in an app whose whole purpose is removing them.
/// Card packs routinely name every file `card.png`, and identical cards across
/// packs are the norm, so this is the common case, not a corner one.
///
/// Prefiltered on `metadata().len()` (usually zero or one candidate), so the
/// byte comparison is not run per claim. The smallest destination path wins
/// among equals purely so the answer does not depend on hash iteration order.
fn claim_with_identical_content(claims: &HashMap<String, Claim>, source: &Path) -> Option<PathBuf> {
    let len = fs::metadata(source).ok()?.len();
    claims
        .values()
        .filter(|c| fs::metadata(&c.source).map(|m| m.len() == len).unwrap_or(false))
        .filter(|c| same_bytes(source, &c.source))
        .map(|c| c.dest.clone())
        .min()
}

/// Resolves where `source` (named `name`, destined for `dir`) should land,
/// checking both the live filesystem AND every name already claimed earlier
/// in this same `plan()` pass — otherwise two incoming files that map to the
/// same destination name would each independently see an empty/differing
/// destination and `apply` would silently clobber one with the other.
/// `claims` is the running set of names already spoken for in `dir`; a
/// successful resolution (`None` or `Renamed`) registers its own claim
/// before returning. `Err` means a collision could not be determined safely
/// (an enumeration failure) — the caller must not plan a move in that case.
fn resolve_collision(
    dir: &Path,
    name_s: &str,
    source: &Path,
    claims: &mut HashMap<String, Claim>,
) -> Result<(PathBuf, Collision), String> {
    let key = claim_key(name_s);
    let occupant = match claims.get(&key) {
        Some(c) => Some((c.dest.clone(), c.source.clone())),
        None => existing_case_insensitive(dir, name_s)?.map(|p| (p.clone(), p)),
    };

    match occupant {
        None => {
            let dest = dir.join(name_s);
            claims.insert(key, Claim { dest: dest.clone(), source: source.to_path_buf() });
            Ok((dest, Collision::None))
        }
        Some((dest, content_ref)) if same_bytes(source, &content_ref) => {
            Ok((dest, Collision::AlreadyFiled))
        }
        Some(_) => {
            // The occupant of the plain name differs — but this content may
            // still be spoken for in this directory under a SUFFIXED name, and
            // suffixing again would file the same bytes twice.
            if let Some(dest) = claim_with_identical_content(claims, source) {
                return Ok((dest, Collision::AlreadyFiled));
            }
            match suffixed(dir, name_s, claims)? {
                Some(dest) => {
                    let cand_name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    claims.insert(claim_key(&cand_name), Claim {
                        dest: dest.clone(),
                        source: source.to_path_buf(),
                    });
                    Ok((dest, Collision::Renamed))
                }
                None => Ok((dir.join(name_s), Collision::Unresolvable)),
            }
        }
    }
}

pub fn apply(plan: &Plan) -> ApplyResult {
    let mut r = ApplyResult::default();
    for m in &plan.moves {
        match m.collision {
            Collision::AlreadyFiled => {
                r.already_filed += 1;
                continue;
            }
            Collision::Unresolvable => {
                r.errors.push(format!(
                    "{}: could not find a free destination name near \"{}\" (every \"name (N)\" slot up to the search cap is taken)",
                    m.from.display(),
                    m.to.display()
                ));
                continue;
            }
            Collision::None | Collision::Renamed => {}
        }
        // The plan said this name was free. Verify it still is, immediately
        // before the rename that would otherwise overwrite whatever is there:
        // `fs::rename` replaces the destination silently on both Windows
        // (MOVEFILE_REPLACE_EXISTING) and POSIX, so any mismatch between plan
        // and reality — a plan gone stale, a file created since, or a planning
        // bug that ever mistakes "unknown" for "free" — destroys a card and
        // reports a clean move. `AlreadyFiled` returned above, so this cannot
        // fire on the legitimate same-content case.
        if m.to.exists() {
            r.errors.push(format!(
                "{} -> {}: the destination already exists although the plan found it free; nothing was moved — re-run the dry-run to re-plan",
                m.from.display(),
                m.to.display()
            ));
            continue;
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
        // rename() fails across volumes; fall back to copy + remove. If the
        // copy lands but the source can't be removed, the card now exists in
        // two places — that must be reported plainly, not folded into a
        // generic "move failed" (the move DID partly succeed).
        let moved = match fs::rename(&m.from, &m.to) {
            Ok(()) => true,
            Err(_) => match fs::copy(&m.from, &m.to) {
                Ok(_) => match fs::remove_file(&m.from) {
                    Ok(()) => true,
                    Err(e) => {
                        r.errors.push(format!(
                            "{} -> {}: copied successfully but the source file could not be removed ({e}); delete {} manually to finish the move",
                            m.from.display(),
                            m.to.display(),
                            m.from.display()
                        ));
                        false
                    }
                },
                Err(e) => {
                    r.errors.push(format!(
                        "{} -> {}: move failed ({e})",
                        m.from.display(),
                        m.to.display()
                    ));
                    false
                }
            },
        };
        if !moved {
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

pub fn plan(root: &Path, recursive: bool, voice: Option<&VoiceSupport>) -> Plan {
    let mut files = Vec::new();
    let mut p = Plan::default();
    walk(root, recursive, &mut files, &mut p.unreadable);
    // Tracks, per destination directory, the names already claimed during
    // THIS pass — the live filesystem alone isn't enough since planning
    // never moves anything; two incoming files bound for the same name must
    // be resolved against each other, not both against an unchanged disk.
    let mut claims: HashMap<PathBuf, HashMap<String, Claim>> = HashMap::new();
    for f in files {
        if is_in_dest_folder(root, &f) {
            // Already-filed cards are exactly the population headed for the
            // user's actual conversion step, so they must still be voice
            // checked — being flagged does not change where a card goes
            // (it stays in `skipped`, never gains a `moves` entry), only
            // what gets reported. Reading is skipped entirely when there is
            // no voice check to make, to leave the no-`voice` behaviour
            // unchanged. A read failure here must not turn a clean `skipped`
            // into an error: it is reported the same way any other
            // unreadable file is, and the skip still happens.
            if voice.is_some() {
                match read_card(&f) {
                    Ok(meta) => {
                        if let Some(pid) = voice_issue(&meta, voice) {
                            p.voice_incompatible.push(VoiceIssue { path: f.clone(), personality: pid });
                        }
                    }
                    Err(e @ CardError::Unrecognized(_)) => {
                        p.unrecognized.push(Unreadable { path: f.clone(), reason: e.reason() });
                    }
                    Err(e) => {
                        p.unreadable.push(Unreadable { path: f.clone(), reason: e.reason() });
                    }
                }
            }
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
        // Only Sunshine cards are headed for conversion into KK, so only they
        // can end up voiceless there. Flagging is a report, not a filter —
        // the card still gets its normal entry in `moves` below.
        if let Some(pid) = voice_issue(&meta, voice) {
            p.voice_incompatible.push(VoiceIssue { path: f.clone(), personality: pid });
        }
        let dir = destination(root, &meta);
        let name_s = name.to_string_lossy().to_string();
        let dir_claims = claims.entry(dir.clone()).or_default();
        match resolve_collision(&dir, &name_s, &f, dir_claims) {
            Ok((to, collision)) => p.moves.push(Planned { from: f, to, collision }),
            Err(reason) => p.unreadable.push(Unreadable { path: f, reason }),
        }
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

    /// Same failure class as the walk-enumeration case above, just hit while
    /// scanning a destination directory for a name collision instead. Must
    /// be reported, never silently folded into "no collision" — that would
    /// let `apply` overwrite an entry it never actually saw.
    #[test]
    fn a_collision_scan_enumeration_failure_is_reported_not_treated_as_no_collision() {
        let dir = PathBuf::from(r"Z:\cardpacks\broken");
        let source = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let reason = collision_scan_error_to_reason(CollisionScanError { dir: dir.clone(), source });
        assert!(reason.contains("name collision"), "{reason}");
        assert!(reason.contains("access denied"), "{reason}");
    }

    /// A destination directory that does not exist yet genuinely has nothing
    /// in it to collide with — that, and only that, is the `Ok(None)` case.
    #[test]
    fn a_missing_destination_directory_is_no_collision() {
        let dir = std::env::temp_dir().join("kdedupe_no_such_dest_dir_ever");
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(existing_case_insensitive(&dir, "c.png").unwrap(), None);
    }

    /// Any OTHER `read_dir` failure (a dropped share, a list-denying ACL) makes
    /// the collision status UNKNOWN and must be an `Err`. Reporting "no
    /// collision" there is what lets `apply` rename over a card it never saw.
    /// A path that is a file rather than a directory is the one such failure
    /// reproducible portably (`ENOTDIR` / `ERROR_DIRECTORY`, never `NotFound`).
    #[test]
    fn a_destination_directory_that_cannot_be_listed_is_unknown_not_no_collision() {
        let base = std::env::temp_dir().join("kdedupe_unlistable_dest");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let not_a_dir = base.join("i_am_a_file");
        fs::write(&not_a_dir, b"x").unwrap();
        let err = existing_case_insensitive(&not_a_dir, "c.png")
            .expect_err("an unlistable directory must not read as \"no collision\"");
        assert!(err.contains("could not be listed"), "{err}");
        assert!(err.contains("c.png"), "{err}");
    }

    /// Windows folds case beyond ASCII: `Ç.png` and `ç.png` are one file. Two
    /// pending sources named that way must land on the same claim key, or each
    /// plans a move to "its own" name and `apply` collapses them into one
    /// silent overwrite.
    #[test]
    fn claim_key_folds_non_ascii_case_too() {
        assert_eq!(claim_key("Ç.png"), claim_key("ç.png"));
        assert_eq!(claim_key("ÄÖÜ.PNG"), claim_key("äöü.png"));
        // ASCII folding must be unchanged.
        assert_eq!(claim_key("C.PNG"), claim_key("c.png"));
    }

    /// If every `name (N)` slot up to the search cap is already claimed,
    /// `suffixed` must report exhaustion rather than falling back to the
    /// original colliding name (which is exactly the overwrite it exists to
    /// prevent). Uses a nonexistent directory so every disk check is `Ok(None)`
    /// and only the `claims` map — filled to capacity — drives the result.
    #[test]
    fn suffixed_exhaustion_is_reported_not_silently_overwritten() {
        let dir = PathBuf::from(r"Z:\cardpacks\nonexistent");
        let mut claims: HashMap<String, Claim> = HashMap::new();
        for n in 2..10_000 {
            claims.insert(claim_key(&format!("c ({n}).png")), Claim {
                dest: PathBuf::new(),
                source: PathBuf::new(),
            });
        }
        let result = suffixed(&dir, "c.png", &claims).unwrap();
        assert!(result.is_none(), "every candidate is claimed; must report exhaustion");
    }

    /// `resolve_collision` must turn that exhaustion into `Collision::Unresolvable`
    /// rather than silently reusing the colliding destination.
    #[test]
    fn resolve_collision_reports_unresolvable_when_every_suffix_is_taken() {
        let dir = PathBuf::from(r"Z:\cardpacks\nonexistent");
        let mut claims: HashMap<String, Claim> = HashMap::new();
        // Something already holds the plain name, with content that will
        // compare as "different" (the fake path doesn't exist, so
        // `same_bytes` reads nothing and returns false).
        claims.insert(claim_key("c.png"), Claim {
            dest: dir.join("c.png"),
            source: PathBuf::from("nonexistent-existing-content"),
        });
        for n in 2..10_000 {
            claims.insert(claim_key(&format!("c ({n}).png")), Claim {
                dest: PathBuf::new(),
                source: PathBuf::new(),
            });
        }
        let source = PathBuf::from("nonexistent-incoming-content");
        let (_, collision) = resolve_collision(&dir, "c.png", &source, &mut claims).unwrap();
        assert_eq!(collision, Collision::Unresolvable);
    }
}
