//! Headless round through the built `kdedupe` binary (CARGO_BIN_EXE_*), mirroring
//! round.rs: 3 byte-identical pairs + 1 unique -> scan(3 groups) -> delete dry-run
//! (deletes nothing) -> delete --apply (deletes 3) -> re-scan(0 groups). This also
//! pins the safety contract: dry-run must not touch the disk. Skips when the
//! local-only testdata fixtures are absent.

mod common;

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

/// organize is dry-run by default — the same safety contract as delete.
#[test]
fn cli_organize_dry_run_then_apply() {
    let tmp = env::temp_dir().join("kdedupe_organize_cli");
    let _ = fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    let deep = root.join("Koikatu_F_20260101000000000_x").join("card");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("c.png"), app_lib_card_fixture()).unwrap();
    let root_s = root.to_str().unwrap();

    // dry-run: reports the move, touches nothing
    let d = json(&["organize", "--root", root_s, "--recursive"]);
    assert_eq!(d["dry_run"], true, "organize must default to dry-run");
    assert_eq!(d["moves"].as_array().unwrap().len(), 1, "the card-pack folder must be seen");
    assert!(deep.join("c.png").exists(), "dry-run must not move anything");

    // apply: files the card
    let a = json(&["organize", "--root", root_s, "--recursive", "--apply"]);
    assert_eq!(a["moved"], 1);
    assert!(root.join("Koikatu").join("Female").join("c.png").exists());
    assert!(!deep.join("c.png").exists());

    // describe advertises the command so an agent can discover it
    let ds = json(&["describe"]);
    let names: Vec<&str> = ds["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"organize"), "describe must list organize, got {names:?}");

    // describe IS the agent contract, so the one place the report's shape is
    // surprising must be stated there: with --game-root the buckets overlap.
    let org = ds["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "organize")
        .expect("organize entry");
    let output = org["output"].as_str().unwrap();
    assert!(
        output.contains("BOTH skipped and unreadable"),
        "describe must warn that the buckets overlap under --game-root, got: {output}"
    );

    // The manifest's workflow advice covered only the dedup path, so an agent
    // reading it had no organize procedure and no warning that --apply here is
    // a move with no Recycle Bin behind it.
    let wf = ds["safe_workflow_organize"].as_array().expect("safe_workflow_organize");
    assert!(
        wf.iter().any(|s| s.as_str().unwrap().contains("--apply")),
        "the organize workflow must end at --apply, got {wf:?}"
    );
    let note = ds["organize_is_a_move"].as_str().expect("organize_is_a_move");
    assert!(note.contains("Recycle Bin"), "the move must be contrasted with delete, got: {note}");
}

/// `organize` must NOT inherit the saved-config root fallback. It is the first
/// mutating command whose only required argument could come from config, so a
/// forgotten --root would bulk-move the GUI's last-used library — for this
/// user, a 161,963-card collection. The read-only commands keep the fallback:
/// that behaviour is unchanged, and the control assertion here proves it.
#[test]
fn cli_organize_requires_an_explicit_root_while_count_still_falls_back() {
    let tmp = env::temp_dir().join("kdedupe_organize_needs_root");
    let _ = fs::remove_dir_all(&tmp);
    let saved_root = tmp.join("saved_library");
    fs::create_dir_all(&saved_root).unwrap();
    let cfg = tmp.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{"root":{},"mode":"byte"}}"#,
            serde_json::to_string(saved_root.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();

    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_kdedupe"))
            .args(args)
            .env("KDEDUPE_CONFIG", &cfg)
            .output()
            .expect("run kdedupe")
    };

    let o = run(&["organize"]);
    assert_eq!(o.status.code(), Some(2), "organize without --root must be a usage error");
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("--root"),
        "the error must name the missing flag, got: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let o2 = run(&["organize", "--apply"]);
    assert_eq!(o2.status.code(), Some(2), "--apply must not bypass the requirement either");

    // Control: an existing read-only command still resolves root from config.
    let c = run(&["count"]);
    assert_eq!(c.status.code(), Some(0), "count must still fall back to the saved root");

    // The resolved root is echoed, so the tree operated on is visible.
    let root_s = saved_root.to_str().unwrap();
    let d = json(&["organize", "--root", root_s]);
    assert_eq!(d["root"], root_s, "dry-run must echo the resolved root");
    let a = json(&["organize", "--root", root_s, "--apply"]);
    assert_eq!(a["root"], root_s, "--apply must echo the resolved root too");
}

/// Same synthesiser the other test crates use — the CLI test drives the built
/// binary but still builds its input in-process.
fn app_lib_card_fixture() -> Vec<u8> {
    common::fixture::card("【KoiKatuChara】", 1, "東山", "涼子", Some(19))
}

/// A `--game-root` that cannot be scanned (bad path) must not be silently
/// treated as "supports nothing" — that would flag every Sunshine card in
/// the tree, exactly the guessed supported-set bug `voice_ok` exists to make
/// visible instead. `voice_ok` must report the failure, and nothing may land
/// in `voice_incompatible` on its account.
#[test]
fn cli_organize_bad_game_root_reports_failure_and_flags_nothing() {
    let tmp = env::temp_dir().join("kdedupe_organize_voice_fail");
    let _ = fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    fs::create_dir_all(&root).unwrap();
    // A Sunshine card with a personality: the population a failed scan would
    // wrongly flag if it were ever mistaken for "install supports nothing".
    fs::write(
        root.join("c.png"),
        common::fixture::card("【KoiKatuCharaSun】", 1, "山田", "花子", Some(5)),
    )
    .unwrap();
    let bad_game_root = tmp.join("no_such_game_install");
    let root_s = root.to_str().unwrap();
    let game_root_s = bad_game_root.to_str().unwrap();

    let d = json(&["organize", "--root", root_s, "--game-root", game_root_s]);
    assert_eq!(d["voice_ok"], false, "an unscannable game root must report a failed scan");
    assert_eq!(
        d["voice_incompatible"].as_array().unwrap().len(),
        0,
        "a failed scan must not flag any card"
    );
}

/// A `--game-root` that CAN be scanned distinguishes a supported personality
/// from an unsupported one: `voice_ok` reports success, and only the card
/// whose personality is absent from the install's `pcm` folders is flagged.
#[test]
fn cli_organize_good_game_root_flags_only_unsupported_personality() {
    let tmp = env::temp_dir().join("kdedupe_organize_voice_ok");
    let _ = fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    fs::create_dir_all(&root).unwrap();

    // A synthesised install that supports only personality 0 — no real game
    // files needed, just the folder shape voice_support() reads.
    let game_root = tmp.join("game");
    fs::create_dir_all(game_root.join("abdata").join("sound").join("data").join("pcm").join("c00")).unwrap();

    fs::write(
        root.join("supported.png"),
        common::fixture::card("【KoiKatuCharaSun】", 1, "東", "支援", Some(0)),
    )
    .unwrap();
    fs::write(
        root.join("unsupported.png"),
        common::fixture::card("【KoiKatuCharaSun】", 1, "西", "非支援", Some(5)),
    )
    .unwrap();

    let root_s = root.to_str().unwrap();
    let game_root_s = game_root.to_str().unwrap();
    let d = json(&["organize", "--root", root_s, "--game-root", game_root_s]);
    assert_eq!(d["voice_ok"], true, "a readable pcm folder is a successful scan");
    let flagged = d["voice_incompatible"].as_array().unwrap();
    assert_eq!(
        flagged.len(),
        1,
        "only the unsupported-personality card should be flagged, got {flagged:?}"
    );
    assert!(
        flagged[0]["path"].as_str().unwrap().contains("unsupported.png"),
        "the flagged card must be the unsupported one, got {flagged:?}"
    );
}
