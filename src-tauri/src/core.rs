//! Pure dedup logic on a SQLite store (same schema as the Python dedupe.py, so
//! the GUI and CLI can share one dedupe.sqlite). Scan top-level pngs, two-tier
//! xxhash (head 1MB -> full) on size-collisions, group by full_hash, delete.
//! Advanced mode hashes the appended Koikatsu block instead (char_len -> char_hash).

use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::hash::Hasher;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Instant;
use twox_hash::XxHash64;

const HEAD: u64 = 1 << 20; // 1 MB

fn group_col(mode: &str) -> &'static str {
    if mode == "char" {
        "char_hash"
    } else {
        "full_hash"
    }
}

/// Progress tick emitted during a long sync so the UI can show it (not freeze).
#[derive(Clone, Serialize)]
pub struct Progress {
    pub phase: String, // "scan" | "head" | "full" | "char-scan" | "char"
    pub done: u64,
    pub total: u64, // 0 = unknown (indeterminate)
}

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub mtime: f64, // unix seconds; used by "auto: keep newest" in the UI
}

#[derive(Serialize)]
pub struct Group {
    pub hash: String,
    pub files: Vec<FileEntry>,
}

pub struct SyncResult {
    pub total: i64,
    pub groups: i64,
    pub dup_files: i64,
    pub new: i64,
    pub pruned: i64,
}

pub fn open_db(db: &Path) -> rusqlite::Result<Connection> {
    if let Some(p) = db.parent() {
        if !p.as_os_str().is_empty() {
            let _ = fs::create_dir_all(p);
        }
    }
    let conn = Connection::open(db)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS files(
            path      TEXT PRIMARY KEY,
            size      INTEGER NOT NULL,
            mtime     REAL    NOT NULL,
            head_hash TEXT,
            full_hash TEXT,
            char_len  INTEGER,
            char_hash TEXT);
         CREATE INDEX IF NOT EXISTS idx_size ON files(size);
         CREATE INDEX IF NOT EXISTS idx_head ON files(size, head_hash);
         CREATE INDEX IF NOT EXISTS idx_full ON files(full_hash);
         CREATE INDEX IF NOT EXISTS idx_charlen ON files(char_len);
         CREATE INDEX IF NOT EXISTS idx_charhash ON files(char_hash);",
    )?;
    // migrate older DBs that predate the char columns (errors = already present)
    let _ = conn.execute("ALTER TABLE files ADD COLUMN char_len INTEGER", []);
    let _ = conn.execute("ALTER TABLE files ADD COLUMN char_hash TEXT", []);
    Ok(conn)
}

fn is_png(p: &Path) -> bool {
    p.extension()
        .map(|x| x.eq_ignore_ascii_case("png"))
        .unwrap_or(false)
}

fn hash_file(path: &Path, max: Option<u64>) -> std::io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut h = XxHash64::with_seed(0);
    let mut buf = vec![0u8; 1 << 20];
    let mut remaining = max;
    loop {
        let cap = match remaining {
            Some(0) => break,
            Some(r) => std::cmp::min(buf.len() as u64, r) as usize,
            None => buf.len(),
        };
        let n = f.read(&mut buf[..cap])?;
        if n == 0 {
            break;
        }
        h.write(&buf[..n]);
        if let Some(r) = remaining.as_mut() {
            *r -= n as u64;
        }
    }
    Ok(format!("{:016x}", h.finish()))
}

/// Walk the leading PNG's chunks to the end of IEND. Everything after that is
/// the appended Koikatsu character data (cover image ignored). Returns
/// (offset_of_char_block, char_len). None if not a PNG / malformed.
/// NOTE: byte-level only — never decodes the embedded name, so non-UTF-8 / special
/// characters in the card data are irrelevant to grouping.
pub fn png_char_block(path: &Path) -> Option<(u64, u64)> {
    let mut f = fs::File::open(path).ok()?;
    let file_len = f.metadata().ok()?.len();
    let mut sig = [0u8; 8];
    f.read_exact(&mut sig).ok()?;
    if sig != [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return None;
    }
    let mut off: u64 = 8;
    loop {
        let mut hdr = [0u8; 8];
        if f.read_exact(&mut hdr).is_err() {
            return None;
        }
        let ln = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        let is_iend = &hdr[4..8] == b"IEND";
        let next = off + 8 + ln + 4; // length + type + data + crc
        if is_iend {
            return Some((next, file_len.saturating_sub(next)));
        }
        if next > file_len {
            return None; // malformed: chunk runs past EOF
        }
        f.seek(SeekFrom::Start(next)).ok()?;
        off = next;
    }
}

fn hash_char_block(path: &Path, offset: u64) -> std::io::Result<String> {
    let mut f = fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut h = XxHash64::with_seed(0);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.write(&buf[..n]);
    }
    Ok(format!("{:016x}", h.finish()))
}

/// top-level pngs only -> INSERT OR IGNORE; prune rows whose file vanished.
/// Uses dir-entry file_type (free on Windows) to avoid a stat per entry; only
/// the kept pngs get a metadata() call (needed for size/mtime).
fn scan(conn: &mut Connection, root: &Path, on: &mut dyn FnMut(Progress)) -> rusqlite::Result<(i64, i64)> {
    let mut seen: Vec<(String, i64, f64)> = Vec::new();
    let mut seen_set: HashSet<String> = HashSet::new();
    let mut last = Instant::now();
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if !is_png(&p) {
                continue;
            }
            // file_type() is free from the directory enumeration on Windows
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let md = match e.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let path = p.to_string_lossy().to_string();
            seen_set.insert(path.clone());
            seen.push((path, md.len() as i64, mtime));
            if last.elapsed().as_millis() >= 150 {
                on(Progress { phase: "scan".into(), done: seen.len() as u64, total: 0 });
                last = Instant::now();
            }
        }
    }
    on(Progress { phase: "scan".into(), done: seen.len() as u64, total: seen.len() as u64 });

    let tx = conn.transaction()?;
    let mut new = 0i64;
    {
        let mut stmt =
            tx.prepare("INSERT OR IGNORE INTO files(path,size,mtime) VALUES(?,?,?)")?;
        for (path, size, mtime) in &seen {
            new += stmt.execute(params![path, size, mtime])? as i64;
        }
    }
    let existing: Vec<String> = {
        let mut s = tx.prepare("SELECT path FROM files")?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        rows.flatten().collect()
    };
    let mut pruned = 0i64;
    {
        let mut del = tx.prepare("DELETE FROM files WHERE path=?")?;
        for p in &existing {
            if !seen_set.contains(p) {
                pruned += del.execute(params![p])? as i64;
            }
        }
    }
    tx.commit()?;
    Ok((new, pruned))
}

/// Commit roughly this often so a kill mid-tier loses at most ~this much work.
/// The candidate SELECT filters on hash IS NULL, so committed files are skipped
/// on the next run — that's what makes a long sync resumable.
const COMMIT_SECS: u64 = 10;

/// run a hashing tier: collect candidate paths, hash each, report progress,
/// committing every COMMIT_SECS so progress survives a close mid-tier.
fn run_tier(
    conn: &mut Connection,
    select_sql: &str,
    update_sql: &str,
    phase: &str,
    hash: impl Fn(&Path) -> Option<String>,
    on: &mut dyn FnMut(Progress),
) -> rusqlite::Result<()> {
    let todo: Vec<String> = {
        let mut s = conn.prepare(select_sql)?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        let v: Vec<String> = rows.flatten().collect();
        v
    };
    let total = todo.len() as u64;
    if total == 0 {
        return Ok(());
    }
    let mut i = 0usize;
    let mut last_progress = Instant::now();
    while i < todo.len() {
        let tx = conn.transaction()?;
        {
            let mut upd = tx.prepare(update_sql)?;
            let batch_start = Instant::now();
            loop {
                if let Some(h) = hash(Path::new(&todo[i])) {
                    upd.execute(params![h, &todo[i]])?;
                }
                i += 1;
                if last_progress.elapsed().as_millis() >= 150 {
                    on(Progress { phase: phase.into(), done: i as u64, total });
                    last_progress = Instant::now();
                }
                if i >= todo.len() || batch_start.elapsed().as_secs() >= COMMIT_SECS {
                    break;
                }
            }
        }
        tx.commit()?; // durable checkpoint
    }
    on(Progress { phase: phase.into(), done: total, total });
    Ok(())
}

fn hash_phase(conn: &mut Connection, on: &mut dyn FnMut(Progress)) -> rusqlite::Result<()> {
    run_tier(
        conn,
        "SELECT path FROM files WHERE head_hash IS NULL
         AND size IN (SELECT size FROM files GROUP BY size HAVING COUNT(*)>1)",
        "UPDATE files SET head_hash=? WHERE path=?",
        "head",
        |p| hash_file(p, Some(HEAD)).ok(),
        on,
    )?;
    conn.execute(
        "UPDATE files SET full_hash=head_hash
         WHERE full_hash IS NULL AND head_hash IS NOT NULL AND size<=?",
        params![HEAD as i64],
    )?;
    run_tier(
        conn,
        "SELECT path FROM files WHERE full_hash IS NULL AND head_hash IS NOT NULL
         AND (size,head_hash) IN (
            SELECT size,head_hash FROM files WHERE head_hash IS NOT NULL
            GROUP BY size,head_hash HAVING COUNT(*)>1)",
        "UPDATE files SET full_hash=? WHERE path=?",
        "full",
        |p| hash_file(p, None).ok(),
        on,
    )
}

/// Advanced mode: tier1 = char_len (cheap chunk-header walk), tier2 = char_hash
/// of the appended block for char_len-collisions. char_len 0 = no KK block.
fn hash_phase_char(conn: &mut Connection, on: &mut dyn FnMut(Progress)) -> rusqlite::Result<()> {
    run_tier(
        conn,
        "SELECT path FROM files WHERE char_len IS NULL",
        "UPDATE files SET char_len=? WHERE path=?",
        "char-scan",
        |p| Some(png_char_block(p).map(|(_, l)| l).unwrap_or(0).to_string()),
        on,
    )?;
    run_tier(
        conn,
        "SELECT path FROM files WHERE char_hash IS NULL AND char_len > 0
         AND char_len IN (SELECT char_len FROM files WHERE char_len > 0
                          GROUP BY char_len HAVING COUNT(*)>1)",
        "UPDATE files SET char_hash=? WHERE path=?",
        "char",
        |p| png_char_block(p).and_then(|(off, _)| hash_char_block(p, off).ok()),
        on,
    )
}

fn stats(conn: &Connection, col: &str) -> rusqlite::Result<(i64, i64, i64)> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    let groups: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM (SELECT {c} FROM files WHERE {c} IS NOT NULL
             GROUP BY {c} HAVING COUNT(*)>1)",
            c = col
        ),
        [],
        |r| r.get(0),
    )?;
    let dup: i64 = conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(c),0) FROM (SELECT COUNT(*) c FROM files
             WHERE {col} IS NOT NULL GROUP BY {col} HAVING c>1)",
            col = col
        ),
        [],
        |r| r.get(0),
    )?;
    Ok((total, groups, dup))
}

/// mode: "byte" (full_hash) or "char" (char_hash). full: wipe table & rebuild.
/// `on` receives progress ticks (throttled ~150ms) for the UI.
pub fn sync(
    root: &Path,
    db: &Path,
    mode: &str,
    full: bool,
    on: &mut dyn FnMut(Progress),
) -> rusqlite::Result<SyncResult> {
    let mut conn = open_db(db)?;
    if full {
        conn.execute("DELETE FROM files", [])?;
    }
    let (new, pruned) = scan(&mut conn, root, on)?;
    if mode == "char" {
        hash_phase_char(&mut conn, on)?;
    } else {
        hash_phase(&mut conn, on)?;
    }
    let (total, groups, dup_files) = stats(&conn, group_col(mode))?;
    Ok(SyncResult { total, groups, dup_files, new, pruned })
}

pub fn list_groups(db: &Path, mode: &str, limit: usize) -> Vec<Group> {
    let conn = match open_db(db) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let col = group_col(mode);
    let lim: i64 = if limit > i64::MAX as usize { -1 } else { limit as i64 }; // sqlite: -1 = no limit
    let hashes: Vec<String> = {
        let mut s = match conn.prepare(&format!(
            "SELECT {c} FROM files WHERE {c} IS NOT NULL
             GROUP BY {c} HAVING COUNT(*)>1 ORDER BY {c} LIMIT ?",
            c = col
        )) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match s.query_map(params![lim], |r| r.get::<_, String>(0)) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        let v: Vec<String> = rows.flatten().collect();
        v
    };
    let mut out = Vec::new();
    for h in hashes {
        let mut s = match conn
            .prepare(&format!("SELECT path,size,mtime FROM files WHERE {col}=? ORDER BY path", col = col))
        {
            Ok(s) => s,
            Err(_) => continue,
        };
        let files: Vec<FileEntry> = {
            let rows = match s.query_map(params![h], |r| {
                let path: String = r.get(0)?;
                let size: i64 = r.get(1)?;
                let mtime: f64 = r.get(2)?;
                let name = Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                Ok(FileEntry { name, path, size: size as u64, mtime })
            }) {
                Ok(rows) => rows,
                Err(_) => continue,
            };
            let v: Vec<FileEntry> = rows.flatten().collect();
            v
        };
        out.push(Group { hash: h, files });
    }
    out
}

pub fn delete_files(root: &Path, db: &Path, names: &[String]) -> (usize, u64, Vec<String>) {
    let conn = match open_db(db) {
        Ok(c) => c,
        Err(e) => return (0, 0, vec![e.to_string()]),
    };
    let mut deleted = 0;
    let mut freed = 0u64;
    let mut errors = Vec::new();
    for name in names {
        let base = Path::new(name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name);
        let p = root.join(base);
        let md = match fs::metadata(&p) {
            Ok(m) => m,
            Err(e) => {
                errors.push(format!("{}: {}", name, e));
                continue;
            }
        };
        // Try the OS Recycle Bin first (local drives -> Windows Recycle Bin, recoverable).
        let _ = trash::delete(&p);
        // Network shares / NAS have no Recycle Bin — the shell can report success without
        // actually removing the file. Verify; if it's still there, hard-delete it. A NAS keeps
        // its own recycle bin / versioning, so the delete stays recoverable on that side.
        if p.exists() {
            if let Err(e) = fs::remove_file(&p) {
                errors.push(format!("{}: {}", name, e));
                continue;
            }
        }
        freed += md.len();
        deleted += 1;
        let _ = conn.execute(
            "DELETE FROM files WHERE path=?",
            params![p.to_string_lossy().to_string()],
        );
    }
    (deleted, freed, errors)
}

/// Extract human-readable strings (>=4 visible chars) from the appended Koikatsu
/// block, for laying two cards in a char-mode group side by side and eyeballing
/// them. Surfaces the "KoiKatuChara" marker, block names (Custom/Coordinate/
/// Parameter/Status/KKEx...), the character name and mod GUIDs.
// ponytail: a `strings`-style scan, NOT a MessagePack parse — dependency-free and
//           robust across KK/KKS/SP/mod variants; may include some binary noise.
//           Upgrade path: real MessagePack decode of the Parameter/KKEx blocks if
//           structured fields are ever needed.
pub fn card_strings(path: &Path) -> Vec<String> {
    let (off, _) = match png_char_block(path) {
        Some(x) => x,
        None => return Vec::new(),
    };
    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    if f.seek(SeekFrom::Start(off)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    fn flush(cur: &mut String, out: &mut Vec<String>) {
        if cur.chars().filter(|c| !c.is_whitespace()).count() >= 4 {
            let s = cur.trim().to_string();
            if out.last() != Some(&s) {
                out.push(s); // collapse consecutive dups
            }
        }
        cur.clear();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        // printable ascii, or any non-ascii that isn't a control / decode error
        let keep = ch != '\u{FFFD}'
            && (ch.is_ascii_graphic() || ch == ' ' || (!ch.is_ascii() && !ch.is_control()));
        if keep {
            cur.push(ch);
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out.truncate(400); // cap noisy/large cards
    out
}

/// Has this mode's hashing run at all? Lets the UI tell "0 dup groups" (synced,
/// no duplicates) apart from "this mode isn't built yet" (switch modes without a
/// matching sync). char fills char_len for every file; byte fills head_hash for
/// any size-collision (always present in a pool that has duplicates).
pub fn mode_hashed(db: &Path, mode: &str) -> bool {
    let conn = match open_db(db) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let col = if mode == "char" { "char_len" } else { "head_hash" };
    conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM files WHERE {col} IS NOT NULL)", col = col),
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|x| x != 0)
    .unwrap_or(false)
}

/// Count top-level pngs using free dir-entry file_type (no stat per file).
pub fn count_pngs(root: &Path) -> usize {
    fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.file_type().map(|t| t.is_file()).unwrap_or(false)
                        && is_png(&e.path())
                })
                .count()
        })
        .unwrap_or(0)
}
