//! Regression tests for the card organize module. Every card is synthesised in
//! the test, so these run anywhere — no local-only fixtures involved.

mod common;

use app_lib::organize;
use common::fixture::card;
use std::fs;
use std::path::{Path, PathBuf};

fn fresh(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("kdedupe_organize_tests").join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, bytes).unwrap();
    p
}

fn kk_female() -> Vec<u8> {
    card("【KoiKatuChara】", 1, "東山", "涼子", Some(19))
}

/// THE defect this module exists to fix. hamster excluded already-organized
/// folders by substring-matching game names against the whole absolute path,
/// so a card pack folder named with Koikatsu's own export convention
/// (`Koikatu_F_<timestamp>_<name>`) was silently skipped.
#[test]
fn a_card_pack_folder_whose_name_merely_starts_with_a_game_name_is_organized() {
    let root = fresh("cardpack");
    let src = put(
        &root,
        "Koikatu_F_20260725003553199_姬野 夜王/card/c.png",
        &kk_female(),
    );
    let p = organize::plan(&root, true);
    assert_eq!(p.moves.len(), 1, "card must be seen, not skipped: {p:?}");
    assert_eq!(p.moves[0].from, src);
    assert_eq!(p.moves[0].to, root.join("Koikatu").join("Female").join("c.png"));
    assert!(p.skipped.is_empty(), "nothing should be skipped here");
}

/// The original intent must survive the fix: a card already filed at the
/// destination is left alone.
#[test]
fn a_card_already_in_a_destination_folder_is_skipped() {
    let root = fresh("already");
    let src = put(&root, "Koikatu/Female/c.png", &kk_female());
    let p = organize::plan(&root, true);
    assert!(p.moves.is_empty(), "already-filed card must not move: {p:?}");
    assert_eq!(p.skipped, vec![src]);
}

/// Bracketed folder names are a real hazard for glob-based APIs (PowerShell
/// `-Path` reads `[kk]` as a character class). A directory walk must not care.
#[test]
fn a_bracketed_folder_name_is_walked_normally() {
    let root = fresh("brackets");
    put(&root, "[kk] 御坂セット/c.png", &kk_female());
    let p = organize::plan(&root, true);
    assert_eq!(p.moves.len(), 1, "bracketed path must be walked: {p:?}");
}

#[test]
fn a_deep_cjk_path_is_walked_normally() {
    let root = fresh("cjk");
    put(&root, "深層/姫野/カード/日本語 folder/c.png", &kk_female());
    let p = organize::plan(&root, true);
    assert_eq!(p.moves.len(), 1, "CJK path must be walked: {p:?}");
}

/// A path past the legacy MAX_PATH must still be walked. Skips itself when the
/// platform refuses to create one, rather than reporting a false failure.
#[test]
fn a_path_longer_than_260_chars_is_walked_normally() {
    let root = fresh("longpath");
    let deep = root.join("x".repeat(120)).join("y".repeat(120));
    if fs::create_dir_all(&deep).is_err() {
        return; // long paths not enabled on this machine
    }
    let f = deep.join("c.png");
    if fs::write(&f, kk_female()).is_err() {
        return;
    }
    assert!(f.to_string_lossy().len() > 260, "fixture must exceed MAX_PATH");
    let p = organize::plan(&root, true);
    assert_eq!(p.moves.len(), 1, "long path must be walked: {p:?}");
}

#[test]
fn an_unrecognized_marker_is_reported_and_not_moved() {
    let root = fresh("unknown");
    put(&root, "scene.png", &card("【KStudio】", 1, "a", "b", Some(1)));
    let p = organize::plan(&root, true);
    assert!(p.moves.is_empty());
    assert_eq!(p.unrecognized.len(), 1);
    assert!(p.unrecognized[0].reason.contains("KStudio"));
}

#[test]
fn a_non_card_png_is_reported_as_unreadable_and_not_moved() {
    let root = fresh("notcard");
    put(&root, "preview.png", b"\x89PNG\r\n\x1a\nnot really a png body");
    let p = organize::plan(&root, true);
    assert!(p.moves.is_empty());
    assert_eq!(p.unreadable.len(), 1, "{p:?}");
}

#[test]
fn non_recursive_mode_ignores_subfolders() {
    let root = fresh("nonrec");
    put(&root, "top.png", &kk_female());
    put(&root, "sub/nested.png", &kk_female());
    let p = organize::plan(&root, false);
    assert_eq!(p.moves.len(), 1, "only the top-level card: {p:?}");
}

/// hamster renamed a colliding card to `c(1).png`, which manufactures exactly
/// the duplicates this app exists to remove. An identical card already at the
/// destination means the content is filed — report it, do not file it twice.
#[test]
fn a_collision_with_identical_content_is_not_filed_twice() {
    let root = fresh("collide_same");
    let bytes = kk_female();
    put(&root, "incoming/c.png", &bytes);
    put(&root, "Koikatu/Female/c.png", &bytes);

    let p = organize::plan(&root, true);
    assert_eq!(p.moves.len(), 1);
    assert!(matches!(p.moves[0].collision, organize::Collision::AlreadyFiled));

    let r = organize::apply(&p);
    assert_eq!(r.already_filed, 1);
    assert_eq!(r.moved, 0);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(
        !root.join("Koikatu/Female/c(1).png").exists(),
        "must not manufacture a duplicate"
    );
}

/// Different content under the same name is a real conflict: keep both.
#[test]
fn a_collision_with_different_content_is_suffixed() {
    let root = fresh("collide_diff");
    put(&root, "incoming/c.png", &kk_female());
    put(&root, "Koikatu/Female/c.png", &card("【KoiKatuChara】", 1, "別", "人", Some(2)));

    let p = organize::plan(&root, true);
    assert!(matches!(p.moves[0].collision, organize::Collision::Renamed));
    let r = organize::apply(&p);
    assert_eq!(r.renamed, 1);
    assert!(root.join("Koikatu/Female/c.png").exists(), "existing card stays");
    assert!(root.join("Koikatu/Female/c (2).png").exists(), "incoming card kept under a new name");
}

#[test]
fn apply_moves_the_card_and_creates_the_destination_folder() {
    let root = fresh("apply_plain");
    let src = put(&root, "Koikatu_F_20260101000000000_x/card/c.png", &kk_female());
    let p = organize::plan(&root, true);
    let r = organize::apply(&p);
    assert_eq!(r.moved, 1, "{:?}", r.errors);
    assert!(!src.exists(), "source must be gone (move, not copy)");
    assert!(root.join("Koikatu/Female/c.png").exists());
}
