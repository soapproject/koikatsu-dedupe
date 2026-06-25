//! Pure dedup logic on a SQLite store (same schema as the Python dedupe.py, so
//! the GUI and CLI can share one dedupe.sqlite). Scan top-level pngs, two-tier
//! xxhash (head 1MB -> full) on size-collisions, group by full_hash, delete.

use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::hash::Hasher;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use twox_hash::XxHash64;

fn group_col(mode: &str) -> &'static str {
    if mode == "char" {
        "char_hash"
    } else {
        "full_hash"
    }
}

const HEAD: u64 = 1 << 20; // 1 MB

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
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
fn scan(conn: &mut Connection, root: &Path) -> rusqlite::Result<(i64, i64)> {
    let mut seen: Vec<(String, i64, f64)> = Vec::new();
    let mut seen_set: HashSet<String> = HashSet::new();
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() || !is_png(&p) {
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
        }
    }
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

fn hash_phase(conn: &mut Connection) -> rusqlite::Result<()> {
    // tier1: head hash for size-collision candidates
    let todo: Vec<String> = {
        let mut s = conn.prepare(
            "SELECT path FROM files WHERE head_hash IS NULL
             AND size IN (SELECT size FROM files GROUP BY size HAVING COUNT(*)>1)",
        )?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        let v: Vec<String> = rows.flatten().collect();
        v
    };
    {
        let tx = conn.transaction()?;
        {
            let mut upd = tx.prepare("UPDATE files SET head_hash=? WHERE path=?")?;
            for path in &todo {
                if let Ok(h) = hash_file(Path::new(path), Some(HEAD)) {
                    upd.execute(params![h, path])?;
                }
            }
        }
        tx.commit()?;
    }
    // if file fits in HEAD, head hash IS the full hash
    conn.execute(
        "UPDATE files SET full_hash=head_hash
         WHERE full_hash IS NULL AND head_hash IS NOT NULL AND size<=?",
        params![HEAD as i64],
    )?;
    // tier2: full hash where (size, head_hash) collides
    let todo2: Vec<String> = {
        let mut s = conn.prepare(
            "SELECT path FROM files WHERE full_hash IS NULL AND head_hash IS NOT NULL
             AND (size,head_hash) IN (
                SELECT size,head_hash FROM files WHERE head_hash IS NOT NULL
                GROUP BY size,head_hash HAVING COUNT(*)>1)",
        )?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        let v: Vec<String> = rows.flatten().collect();
        v
    };
    {
        let tx = conn.transaction()?;
        {
            let mut upd = tx.prepare("UPDATE files SET full_hash=? WHERE path=?")?;
            for path in &todo2 {
                if let Ok(h) = hash_file(Path::new(path), None) {
                    upd.execute(params![h, path])?;
                }
            }
        }
        tx.commit()?;
    }
    Ok(())
}

/// Advanced mode: tier1 = char_len (cheap chunk-header walk), tier2 = char_hash
/// of the appended block for char_len-collisions. char_len 0 = no KK block.
fn hash_phase_char(conn: &mut Connection) -> rusqlite::Result<()> {
    // tier1: char_len for every file missing it
    let todo: Vec<String> = {
        let mut s = conn.prepare("SELECT path FROM files WHERE char_len IS NULL")?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        let v: Vec<String> = rows.flatten().collect();
        v
    };
    {
        let tx = conn.transaction()?;
        {
            let mut upd = tx.prepare("UPDATE files SET char_len=? WHERE path=?")?;
            for path in &todo {
                let clen = png_char_block(Path::new(path))
                    .map(|(_, l)| l as i64)
                    .unwrap_or(0);
                upd.execute(params![clen, path])?;
            }
        }
        tx.commit()?;
    }
    // tier2: char_hash where char_len collides
    let todo2: Vec<String> = {
        let mut s = conn.prepare(
            "SELECT path FROM files WHERE char_hash IS NULL AND char_len > 0
             AND char_len IN (SELECT char_len FROM files WHERE char_len > 0
                              GROUP BY char_len HAVING COUNT(*)>1)",
        )?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        let v: Vec<String> = rows.flatten().collect();
        v
    };
    {
        let tx = conn.transaction()?;
        {
            let mut upd = tx.prepare("UPDATE files SET char_hash=? WHERE path=?")?;
            for path in &todo2 {
                if let Some((off, _)) = png_char_block(Path::new(path)) {
                    if let Ok(h) = hash_char_block(Path::new(path), off) {
                        upd.execute(params![h, path])?;
                    }
                }
            }
        }
        tx.commit()?;
    }
    Ok(())
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
pub fn sync(root: &Path, db: &Path, mode: &str, full: bool) -> rusqlite::Result<SyncResult> {
    let mut conn = open_db(db)?;
    if full {
        conn.execute("DELETE FROM files", [])?;
    }
    let (new, pruned) = scan(&mut conn, root)?;
    if mode == "char" {
        hash_phase_char(&mut conn)?;
    } else {
        hash_phase(&mut conn)?;
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
            .prepare(&format!("SELECT path,size FROM files WHERE {col}=? ORDER BY path", col = col))
        {
            Ok(s) => s,
            Err(_) => continue,
        };
        let files: Vec<FileEntry> = {
            let rows = match s.query_map(params![h], |r| {
                let path: String = r.get(0)?;
                let size: i64 = r.get(1)?;
                let name = Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                Ok(FileEntry { name, path, size: size as u64 })
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
        match fs::metadata(&p) {
            Ok(md) => match fs::remove_file(&p) {
                Ok(_) => {
                    freed += md.len();
                    deleted += 1;
                    let _ = conn.execute(
                        "DELETE FROM files WHERE path=?",
                        params![p.to_string_lossy().to_string()],
                    );
                }
                Err(e) => errors.push(format!("{}: {}", name, e)),
            },
            Err(e) => errors.push(format!("{}: {}", name, e)),
        }
    }
    (deleted, freed, errors)
}

pub fn count_pngs(root: &Path) -> usize {
    fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    let p = e.path();
                    p.is_file() && is_png(&p)
                })
                .count()
        })
        .unwrap_or(0)
}
