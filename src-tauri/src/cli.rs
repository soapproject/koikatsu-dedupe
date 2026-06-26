//! Headless CLI sharing the GUI's dedup core, so an AI agent (or a human) can
//! drive scan / list / delete from a terminal and read JSON on stdout. Console
//! subsystem (no `windows_subsystem="windows"`), so stdout works in a shell.
//! ponytail: hand-rolled arg parse; switch to clap past ~a dozen subcommands.

#[path = "core.rs"]
mod core;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// Flags that take no value (presence = true). Everything else is `--name value`.
const BOOL_FLAGS: &[&str] = &["full", "apply"];

const USAGE: &str = "\
kdedupe — headless dedup CLI (same dedupe.sqlite as the GUI)

USAGE:
  kdedupe <command> [--flags] [NAMES...]

COMMANDS:
  scan   --root DIR [--db P] [--mode byte|char] [--full]   scan+hash; prints counts
  groups [--db P] [--mode byte|char] [--limit N]           list duplicate groups (JSON)
  stats  [--db P] [--mode byte|char]                       group/dup-file counts
  strings --path PNG                                       readable card strings (JSON)
  count  --root DIR                                         number of top-level pngs
  delete --root DIR [--db P] NAME...                        DRY-RUN; add --apply to delete
  describe                                                  machine-readable manifest (JSON)
  --help | --version

Defaults: --mode byte; --db = %APPDATA%/io.github.soapproject.koikatsu-dedupe/dedupe.sqlite
delete is dry-run by default; --apply sends files to the Recycle Bin (recoverable).";

fn parse(args: &[String]) -> (Vec<String>, HashMap<String, String>) {
    let mut pos = Vec::new();
    let mut flags = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--") {
            if BOOL_FLAGS.contains(&name) {
                flags.insert(name.to_string(), "1".into());
            } else if i + 1 < args.len() {
                flags.insert(name.to_string(), args[i + 1].clone());
                i += 1;
            } else {
                flags.insert(name.to_string(), String::new());
            }
        } else {
            pos.push(a.clone());
        }
        i += 1;
    }
    (pos, flags)
}

fn default_db() -> PathBuf {
    // Mirror the GUI's app_data_dir so the CLI hits the same library by default.
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    Path::new(&base)
        .join("io.github.soapproject.koikatsu-dedupe")
        .join("dedupe.sqlite")
}

fn out(v: Value) {
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
}

fn describe(db: &Path) -> Value {
    json!({
        "tool": "kdedupe",
        "version": env!("CARGO_PKG_VERSION"),
        "summary": "Headless Koikatsu card deduplicator. Shares dedupe.sqlite with the GUI.",
        "default_db": db.to_string_lossy(),
        "modes": ["byte", "char"],
        "safe_workflow": ["scan", "groups", "(agent picks names to delete, keeping 1 per group)", "delete (dry-run)", "delete --apply"],
        "commands": [
            {"name":"scan","args":[{"name":"--root","required":true,"type":"dir"},{"name":"--db","type":"path"},{"name":"--mode","type":"byte|char","default":"byte"},{"name":"--full","type":"bool"}],"output":"{total,groups,dup_files,new,pruned}"},
            {"name":"groups","args":[{"name":"--db","type":"path"},{"name":"--mode","type":"byte|char","default":"byte"},{"name":"--limit","type":"int"}],"output":"[{hash,files:[{name,path,size,mtime}]}]"},
            {"name":"stats","args":[{"name":"--db","type":"path"},{"name":"--mode","type":"byte|char","default":"byte"}],"output":"{groups,dup_files,synced}"},
            {"name":"strings","args":[{"name":"--path","required":true,"type":"png"}],"output":"[string]"},
            {"name":"count","args":[{"name":"--root","required":true,"type":"dir"}],"output":"int"},
            {"name":"delete","args":[{"name":"--root","required":true,"type":"dir"},{"name":"--db","type":"path"},{"name":"NAME...","required":true,"type":"filename[]"},{"name":"--apply","type":"bool"}],"output":"dry-run: {dry_run,would_delete,count}; --apply: {deleted,freed,errors}"}
        ]
    })
}

fn need<'a>(flags: &'a HashMap<String, String>, k: &str, cmd: &str) -> Result<&'a String, i32> {
    flags.get(k).ok_or_else(|| {
        eprintln!("error: {cmd} needs --{k}");
        2
    })
}

pub fn run(argv: &[String]) -> i32 {
    let (pos, flags) = parse(argv);
    let cmd = pos.first().map(|s| s.as_str()).unwrap_or("");
    let db = flags.get("db").map(PathBuf::from).unwrap_or_else(default_db);
    let mode = flags.get("mode").cloned().unwrap_or_else(|| "byte".into());

    match cmd {
        "scan" => {
            let root = match need(&flags, "root", "scan") {
                Ok(r) => PathBuf::from(r),
                Err(c) => return c,
            };
            let full = flags.contains_key("full");
            let mut on = |p: core::Progress| eprintln!("[{}] {}/{}", p.phase, p.done, p.total);
            match core::sync(&root, &db, &mode, full, &mut on) {
                Ok(r) => {
                    out(json!({"total":r.total,"groups":r.groups,"dup_files":r.dup_files,"new":r.new,"pruned":r.pruned}));
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        "groups" => {
            let limit = flags
                .get("limit")
                .and_then(|s| s.parse().ok())
                .unwrap_or(usize::MAX);
            out(serde_json::to_value(core::list_groups(&db, &mode, limit)).unwrap());
            0
        }
        "stats" => {
            let g = core::list_groups(&db, &mode, usize::MAX);
            let dup: usize = g.iter().map(|x| x.files.len()).sum();
            out(json!({"groups":g.len(),"dup_files":dup,"synced":core::mode_hashed(&db,&mode)}));
            0
        }
        "strings" => {
            let path = match need(&flags, "path", "strings") {
                Ok(p) => p,
                Err(c) => return c,
            };
            out(serde_json::to_value(core::card_strings(Path::new(path))).unwrap());
            0
        }
        "count" => {
            let root = match need(&flags, "root", "count") {
                Ok(r) => r,
                Err(c) => return c,
            };
            out(json!(core::count_pngs(Path::new(root))));
            0
        }
        "delete" => {
            let root = match need(&flags, "root", "delete") {
                Ok(r) => PathBuf::from(r),
                Err(c) => return c,
            };
            let names: Vec<String> = pos[1..].to_vec();
            if names.is_empty() {
                eprintln!("error: delete needs at least one NAME");
                return 2;
            }
            if flags.contains_key("apply") {
                let (deleted, freed, errors) = core::delete_files(&root, &db, &names);
                out(json!({"deleted":deleted,"freed":freed,"errors":errors}));
                if errors.is_empty() {
                    0
                } else {
                    1
                }
            } else {
                out(json!({"dry_run":true,"would_delete":names,"count":names.len(),"hint":"re-run with --apply to delete (to Recycle Bin)"}));
                0
            }
        }
        "describe" => {
            out(describe(&db));
            0
        }
        "--version" | "version" => {
            println!("kdedupe {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "" => {
            eprintln!("{USAGE}");
            2
        }
        "help" | "--help" | "-h" => {
            eprintln!("{USAGE}");
            0
        }
        other => {
            eprintln!("error: unknown command '{other}'\n\n{USAGE}");
            2
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&argv));
}
