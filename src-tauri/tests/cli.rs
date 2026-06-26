//! Headless round through the built `kdedupe` binary (CARGO_BIN_EXE_*), mirroring
//! round.rs: 3 byte-identical pairs + 1 unique -> scan(3 groups) -> delete dry-run
//! (deletes nothing) -> delete --apply (deletes 3) -> re-scan(0 groups). This also
//! pins the safety contract: dry-run must not touch the disk. Skips when the
//! local-only testdata fixtures are absent.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

fn kdedupe(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kdedupe"))
        .args(args)
        // Isolate from any real GUI config.json so --root/--db/--mode fallback
        // can't leak the developer's actual library into the test.
        .env("KDEDUPE_CONFIG", "__kdedupe_no_such_config__.json")
        .output()
        .expect("run kdedupe")
}

fn json(args: &[&str]) -> Value {
    let out = kdedupe(args);
    assert!(
        out.status.success(),
        "kdedupe {args:?} exited {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "non-JSON stdout from {args:?}: {e}\n{}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

fn count_png(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
        .count()
}

#[test]
fn cli_round_on_testdata() {
    let td = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("testdata");
    let bases = ["KK_387575.png", "KK_387576.png", "KK_381570.png"];
    let unique = "KK_000997.png";
    if !td.join(bases[0]).exists() || !td.join(unique).exists() {
        return; // fixtures are local-only (real cards, not shipped)
    }

    let tmp = env::temp_dir().join("kdedupe_cli_test");
    let _ = fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    fs::create_dir_all(&root).unwrap();
    for b in bases {
        fs::copy(td.join(b), root.join(b)).unwrap();
        fs::copy(td.join(b), root.join(format!("dup_{b}"))).unwrap(); // byte-identical partner
    }
    fs::copy(td.join(unique), root.join("unique_extra.png")).unwrap();
    let db = tmp.join("cli.sqlite");
    let (root_s, db_s) = (root.to_str().unwrap(), db.to_str().unwrap());

    // scan -> 3 byte groups, 6 dup files
    let r = json(&["scan", "--root", root_s, "--db", db_s, "--mode", "byte"]);
    assert_eq!(r["groups"], 3, "scan groups");
    assert_eq!(r["dup_files"], 6, "scan dup_files");

    // groups -> 3
    let g = json(&["groups", "--db", db_s, "--mode", "byte"]);
    assert_eq!(g.as_array().unwrap().len(), 3, "list groups");

    // delete dry-run: announces, deletes nothing
    let names = ["dup_KK_387575.png", "dup_KK_387576.png", "dup_KK_381570.png"];
    let dr = json(&["delete", "--root", root_s, "--db", db_s, names[0], names[1], names[2]]);
    assert_eq!(dr["dry_run"], true, "dry-run flag");
    assert_eq!(count_png(&root), 7, "dry-run must NOT delete");

    // delete --apply: removes the 3 partners
    let ap = json(&["delete", "--root", root_s, "--db", db_s, "--apply", names[0], names[1], names[2]]);
    assert_eq!(ap["deleted"], 3, "applied deletions");
    assert_eq!(count_png(&root), 4, "files left after apply");

    // re-scan -> no duplicates remain
    let r2 = json(&["scan", "--root", root_s, "--db", db_s, "--mode", "byte"]);
    assert_eq!(r2["groups"], 0, "re-scan groups");

    // usage errors -> exit 2 (not 0, not a panic)
    assert_eq!(kdedupe(&["scan"]).status.code(), Some(2), "missing --root");
    assert_eq!(kdedupe(&["frobnicate"]).status.code(), Some(2), "unknown command");
    assert_eq!(kdedupe(&[]).status.code(), Some(2), "no args -> usage error");

    // help/version are conventional flags (not value-flags) -> exit 0
    let v = kdedupe(&["--version"]);
    assert_eq!(v.status.code(), Some(0), "--version exit");
    assert!(
        String::from_utf8_lossy(&v.stdout).starts_with("kdedupe "),
        "--version prints the version, got: {}",
        String::from_utf8_lossy(&v.stdout)
    );
    assert_eq!(kdedupe(&["--help"]).status.code(), Some(0), "--help exit");
}

/// --root/--db/--mode fall back to the GUI's config.json when the flag is omitted.
/// Hermetic: writes its own config and points $KDEDUPE_CONFIG at it.
#[test]
fn cli_config_fallback() {
    let tmp = env::temp_dir().join("kdedupe_cfg_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let cfg = tmp.join("config.json");
    fs::write(
        &cfg,
        r#"{"root":"R:\\some\\root","db":"R:\\some\\lib.sqlite","mode":"char"}"#,
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_kdedupe"))
        .args(["config"])
        .env("KDEDUPE_CONFIG", &cfg)
        .output()
        .expect("run kdedupe");
    assert!(out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["resolved"]["mode"], "char", "mode from config");
    assert_eq!(v["resolved"]["db"], "R:\\some\\lib.sqlite", "db from config");
    assert_eq!(v["resolved"]["root"], "R:\\some\\root", "root from config");

    // explicit flag still wins over the saved config
    let out2 = Command::new(env!("CARGO_BIN_EXE_kdedupe"))
        .args(["config", "--mode", "byte"])
        .env("KDEDUPE_CONFIG", &cfg)
        .output()
        .expect("run kdedupe");
    let v2: Value = serde_json::from_slice(&out2.stdout).unwrap();
    assert_eq!(v2["resolved"]["mode"], "byte", "flag overrides config");
}
