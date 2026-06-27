//! End-to-end "round" on the project's testdata, mirroring the acceptance check:
//! sync -> 3 dup groups -> delete 2nd of each -> re-sync 0 groups -> 4 files left.

use std::fs;
use std::path::PathBuf;

#[test]
fn round_on_testdata() {
    let proj = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let testdata = proj.join("testdata");
    // Build a deterministic fixture from a few stable single cards: 3 byte-identical
    // pairs + 1 unique. This does NOT depend on the testdata folder's (mutable) dup
    // layout, so pointing the app at testdata and deleting partners can't break it.
    let bases = ["KK_387575.png", "KK_387576.png", "KK_381570.png"];
    let unique = "Koikatu_F_20220608135028452_Haruno Chika_Darycsan20.png";
    if !testdata.join(bases[0]).exists() {
        return; // fixtures are local-only (real cards, not shipped)
    }

    let tmp = std::env::temp_dir().join("dedup_round_rs");
    let _ = fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    fs::create_dir_all(&root).unwrap();
    for b in bases {
        let src = testdata.join(b);
        if !src.exists() {
            return;
        }
        fs::copy(&src, root.join(b)).unwrap();
        fs::copy(&src, root.join(format!("dup_{b}"))).unwrap(); // byte-identical partner
    }
    let uq = testdata.join(unique);
    if !uq.exists() {
        return;
    }
    fs::copy(&uq, root.join("unique_extra.png")).unwrap();
    let db = tmp.join("test.sqlite");

    // 1) byte sync -> 3 dup groups
    let r = app_lib::core::sync(&root, &db, "byte", false, false, &mut |_| {}).unwrap();
    assert_eq!(r.groups, 3, "expected 3 byte groups, got {}", r.groups);

    // 2) list groups
    let groups = app_lib::core::list_groups(&db, "byte", 100);
    assert_eq!(groups.len(), 3);

    // hash output is canonical xxHash64 — pinned regression value
    let g = groups
        .iter()
        .find(|g| g.files.iter().any(|f| f.name == "KK_387575.png"))
        .expect("group containing KK_387575.png");
    assert_eq!(g.hash, "438d100a7685b5a4", "xxHash64 output changed");

    // char parser: appended KK block length must match the python probe
    let (_, clen) = app_lib::core::png_char_block(&testdata.join("KK_387575.png"))
        .expect("KK_387575.png should have a char block");
    assert_eq!(clen, 116364, "char block length parser");

    // detail view: readable strings must surface the KK marker (anchors the parse)
    let strs = app_lib::core::card_strings(&testdata.join("KK_387575.png"));
    assert!(
        strs.iter().any(|s| s.contains("KoiKatu")),
        "card_strings should surface the KoiKatu marker, got {:?}",
        &strs[..strs.len().min(6)]
    );

    // mode_hashed reflects which tiers have run (UI: "not built yet" vs "0 dups")
    assert!(app_lib::core::mode_hashed(&db, "byte"), "byte should read as hashed after byte sync");

    // advanced (char-data) mode also finds the 3 dup groups on this fixture
    let rc = app_lib::core::sync(&root, &db, "char", false, false, &mut |_| {}).unwrap();
    assert_eq!(rc.groups, 3, "expected 3 char groups, got {}", rc.groups);
    assert!(app_lib::core::mode_hashed(&db, "char"), "char should read as hashed after char sync");
    assert_eq!(app_lib::core::list_groups(&db, "char", 100).len(), 3);

    // 3) delete 2nd file of each group (keep first)
    let dels: Vec<String> = groups.iter().map(|g| g.files[1].name.clone()).collect();
    let (deleted, freed, errors) = app_lib::core::delete_files(&root, &db, &dels);
    assert_eq!(deleted, 3);
    assert!(errors.is_empty(), "delete errors: {:?}", errors);
    assert!(freed > 0);

    // 4) re-sync -> 0 dup groups
    let r2 = app_lib::core::sync(&root, &db, "byte", false, false, &mut |_| {}).unwrap();
    assert_eq!(r2.groups, 0, "expected 0 groups after dedup, got {}", r2.groups);

    // 5) 4 files remain (3 kept + 1 unique)
    let left = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
        .count();
    assert_eq!(left, 4, "expected 4 files left, got {}", left);
}

/// A read-only card must still delete. On a NAS (no Recycle Bin) `trash` no-ops,
/// so delete falls through to `fs::remove_file`, which on Windows REFUSES
/// read-only files ("Access is denied"). Regression for the production bug where
/// 25 read-only cards could never be deleted.
#[test]
fn delete_readonly_file() {
    let tmp = std::env::temp_dir().join("dedup_ro_rs");
    let _ = fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    fs::create_dir_all(&root).unwrap();
    let f = root.join("readonly.png");
    fs::write(&f, b"x").unwrap();
    let mut perm = fs::metadata(&f).unwrap().permissions();
    perm.set_readonly(true);
    fs::set_permissions(&f, perm).unwrap();

    let db = tmp.join("ro.sqlite");
    let (deleted, _freed, errors) =
        app_lib::core::delete_files(&root, &db, &["readonly.png".to_string()]);
    assert!(errors.is_empty(), "read-only delete errored: {:?}", errors);
    assert_eq!(deleted, 1);
    assert!(!f.exists(), "read-only file should be gone");
}

/// Recursive scan + the name-collision case it unlocks: two cards with the SAME
/// basename in different subfolders must (a) only both be seen when --recursive,
/// and (b) delete the RIGHT one by full path — not the top-level namesake.
#[test]
fn recursive_scan_collision_delete() {
    let tmp = std::env::temp_dir().join("dedup_recursive_rs");
    let _ = fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    fs::create_dir_all(root.join("sub")).unwrap();
    let body = b"identical-card-bytes-xxxxxxxxxxxxxxxx";
    fs::write(root.join("a.png"), body).unwrap(); // top-level
    fs::write(root.join("sub").join("a.png"), body).unwrap(); // same name, same bytes, nested
    let db = tmp.join("rec.sqlite");

    // non-recursive: only the top-level a.png is seen -> no dup group
    let r = app_lib::core::sync(&root, &db, "byte", true, false, &mut |_| {}).unwrap();
    assert_eq!(r.total, 1, "non-recursive should see only the top-level png");
    assert_eq!(r.groups, 0);

    // recursive: both seen -> 1 byte-identical group of 2
    let rr = app_lib::core::sync(&root, &db, "byte", true, true, &mut |_| {}).unwrap();
    assert_eq!(rr.total, 2, "recursive should see both pngs");
    assert_eq!(rr.groups, 1);

    let groups = app_lib::core::list_groups(&db, "byte", 10);
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!(g.files.len(), 2);
    assert!(g.files.iter().all(|f| f.name == "a.png"), "both basenames collide");

    // delete the NESTED one by full path; the top-level namesake must survive
    let nested = g.files.iter().find(|f| f.path.contains("sub")).unwrap().path.clone();
    let (deleted, freed, errors) = app_lib::core::delete_files(&root, &db, &[nested]);
    assert_eq!(deleted, 1, "delete errors: {:?}", errors);
    assert!(errors.is_empty());
    assert!(freed > 0);
    assert!(root.join("a.png").exists(), "top-level namesake must NOT be deleted");
    assert!(!root.join("sub").join("a.png").exists(), "nested card should be gone");
}

/// Large modded cards, one with a non-ASCII (CJK) filename: prove the path
/// round-trips through scan -> hash -> group -> delete in both modes.
#[test]
fn large_nonascii_round() {
    let proj = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let large = proj.join("testdata").join("large");
    if !large.exists() {
        return; // fixtures optional
    }
    let tmp = std::env::temp_dir().join("dedup_large_rs");
    let _ = fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    fs::create_dir_all(&root).unwrap();
    for e in fs::read_dir(&large).unwrap().flatten() {
        let p = e.path();
        if p.is_file()
            && p.extension()
                .map(|x| x.eq_ignore_ascii_case("png"))
                .unwrap_or(false)
        {
            fs::copy(&p, root.join(p.file_name().unwrap())).unwrap();
        }
    }
    let db = tmp.join("large.sqlite");

    // one big dup pair -> 1 group in both modes
    assert_eq!(app_lib::core::sync(&root, &db, "byte", false, false, &mut |_| {}).unwrap().groups, 1);
    assert_eq!(app_lib::core::sync(&root, &db, "char", false, false, &mut |_| {}).unwrap().groups, 1);

    let groups = app_lib::core::list_groups(&db, "byte", 10);
    assert_eq!(groups.len(), 1);
    // files sorted by path: the CJK-named one sorts after the ASCII one
    let target = groups[0].files[1].name.clone();
    assert!(!target.is_ascii(), "expected non-ASCII filename, got {}", target);

    // delete the non-ASCII-named card -> must succeed
    let (deleted, freed, errors) = app_lib::core::delete_files(&root, &db, &[target]);
    assert_eq!(deleted, 1, "non-ASCII delete failed: {:?}", errors);
    assert!(freed > 0);
    assert_eq!(app_lib::core::sync(&root, &db, "byte", false, false, &mut |_| {}).unwrap().groups, 0);
}
