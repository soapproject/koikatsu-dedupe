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
    let p = organize::plan(&root, true, None);
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
    let p = organize::plan(&root, true, None);
    assert!(p.moves.is_empty(), "already-filed card must not move: {p:?}");
    assert_eq!(p.skipped, vec![src]);
}

/// Bracketed folder names are a real hazard for glob-based APIs (PowerShell
/// `-Path` reads `[kk]` as a character class). A directory walk must not care.
#[test]
fn a_bracketed_folder_name_is_walked_normally() {
    let root = fresh("brackets");
    put(&root, "[kk] 御坂セット/c.png", &kk_female());
    let p = organize::plan(&root, true, None);
    assert_eq!(p.moves.len(), 1, "bracketed path must be walked: {p:?}");
}

#[test]
fn a_deep_cjk_path_is_walked_normally() {
    let root = fresh("cjk");
    put(&root, "深層/姫野/カード/日本語 folder/c.png", &kk_female());
    let p = organize::plan(&root, true, None);
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
    let p = organize::plan(&root, true, None);
    assert_eq!(p.moves.len(), 1, "long path must be walked: {p:?}");
}

#[test]
fn an_unrecognized_marker_is_reported_and_not_moved() {
    let root = fresh("unknown");
    put(&root, "scene.png", &card("【KStudio】", 1, "a", "b", Some(1)));
    let p = organize::plan(&root, true, None);
    assert!(p.moves.is_empty());
    assert_eq!(p.unrecognized.len(), 1);
    assert!(p.unrecognized[0].reason.contains("KStudio"));
}

#[test]
fn a_non_card_png_is_reported_as_unreadable_and_not_moved() {
    let root = fresh("notcard");
    put(&root, "preview.png", b"\x89PNG\r\n\x1a\nnot really a png body");
    let p = organize::plan(&root, true, None);
    assert!(p.moves.is_empty());
    assert_eq!(p.unreadable.len(), 1, "{p:?}");
}

#[test]
fn non_recursive_mode_ignores_subfolders() {
    let root = fresh("nonrec");
    put(&root, "top.png", &kk_female());
    put(&root, "sub/nested.png", &kk_female());
    let p = organize::plan(&root, false, None);
    assert_eq!(p.moves.len(), 1, "only the top-level card: {p:?}");
}

/// hamster renamed a colliding card to `c(1).png`, which manufactures exactly
/// the duplicates this app exists to remove. An identical card already at the
/// destination means the content is filed — report it, do not file it twice.
#[test]
fn a_collision_with_identical_content_is_not_filed_twice() {
    let root = fresh("collide_same");
    let bytes = kk_female();
    let incoming = put(&root, "incoming/c.png", &bytes);
    put(&root, "Koikatu/Female/c.png", &bytes);

    let p = organize::plan(&root, true, None);
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
    assert!(
        incoming.exists(),
        "AlreadyFiled must not touch the source: nothing is copied, nothing is deleted"
    );
}

/// Different content under the same name is a real conflict: keep both.
#[test]
fn a_collision_with_different_content_is_suffixed() {
    let root = fresh("collide_diff");
    put(&root, "incoming/c.png", &kk_female());
    put(&root, "Koikatu/Female/c.png", &card("【KoiKatuChara】", 1, "別", "人", Some(2)));

    let p = organize::plan(&root, true, None);
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
    let p = organize::plan(&root, true, None);
    let r = organize::apply(&p);
    assert_eq!(r.moved, 1, "{:?}", r.errors);
    assert!(!src.exists(), "source must be gone (move, not copy)");
    assert!(root.join("Koikatu/Female/c.png").exists());
}

/// THE Critical fix: planning never touches the filesystem, only `apply`
/// does. Two incoming files that map to the SAME destination name must be
/// resolved against each other during `plan`, not each independently against
/// an unchanged disk — otherwise `apply`'s `fs::rename` silently clobbers the
/// first with the second and `ApplyResult.errors` stays empty.
#[test]
fn two_pending_sources_with_identical_content_at_the_same_destination_are_filed_once() {
    let root = fresh("pending_same");
    let bytes = kk_female();
    let a = put(&root, "A/c.png", &bytes);
    let b = put(&root, "B/c.png", &bytes);

    let p = organize::plan(&root, true, None);
    assert_eq!(p.moves.len(), 2, "{p:?}");
    assert!(matches!(p.moves[0].collision, organize::Collision::None));
    assert!(matches!(p.moves[1].collision, organize::Collision::AlreadyFiled));
    assert_eq!(
        p.moves[0].to, p.moves[1].to,
        "both plan entries must target the same destination name"
    );

    let r = organize::apply(&p);
    assert_eq!(r.moved, 1, "{:?}", r.errors);
    assert_eq!(r.already_filed, 1);
    assert!(r.errors.is_empty(), "{:?}", r.errors);

    assert!(!a.exists(), "the filed copy's source is gone (moved)");
    assert!(b.exists(), "the already-filed copy's source is untouched");
    assert!(root.join("Koikatu/Female/c.png").exists());
    assert_eq!(fs::read(root.join("Koikatu/Female/c.png")).unwrap(), bytes);
    assert!(
        !root.join("Koikatu/Female/c (2).png").exists(),
        "identical content must not manufacture a second copy under a new name"
    );
}

/// Same setup, but the two pending sources genuinely differ: both must
/// survive, under distinct names, exactly as a same-name-on-disk conflict
/// already does — the fix must not merge or drop either one.
#[test]
fn two_pending_sources_with_different_content_at_the_same_destination_are_both_kept() {
    let root = fresh("pending_diff");
    let bytes_a = kk_female();
    let bytes_b = card("【KoiKatuChara】", 1, "別", "人", Some(2));
    let a = put(&root, "A/c.png", &bytes_a);
    let b = put(&root, "B/c.png", &bytes_b);

    let p = organize::plan(&root, true, None);
    assert_eq!(p.moves.len(), 2, "{p:?}");
    assert!(matches!(p.moves[0].collision, organize::Collision::None));
    assert!(matches!(p.moves[1].collision, organize::Collision::Renamed));
    assert_ne!(
        p.moves[0].to, p.moves[1].to,
        "differing content must land under distinct names"
    );

    let r = organize::apply(&p);
    assert_eq!(r.moved, 1, "{:?}", r.errors);
    assert_eq!(r.renamed, 1, "{:?}", r.errors);
    assert!(r.errors.is_empty(), "{:?}", r.errors);

    assert!(!a.exists() && !b.exists(), "both sources moved");
    assert_eq!(fs::read(root.join("Koikatu/Female/c.png")).unwrap(), bytes_a);
    assert_eq!(fs::read(root.join("Koikatu/Female/c (2).png")).unwrap(), bytes_b);
}

/// `Collision::Unresolvable` (every suffix slot exhausted) must be reported
/// as an error and must never touch the filesystem — not moved, not
/// counted, and the destination it names must stay untouched.
#[test]
fn apply_reports_unresolvable_as_an_error_without_touching_the_filesystem() {
    let root = fresh("unresolvable");
    let from = put(&root, "incoming.png", &kk_female());
    let to = root.join("Koikatu/Female/c.png");
    let p = organize::Plan {
        moves: vec![organize::Planned { from: from.clone(), to: to.clone(), collision: organize::Collision::Unresolvable }],
        ..Default::default()
    };

    let r = organize::apply(&p);
    assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
    assert_eq!(r.moved, 0);
    assert_eq!(r.renamed, 0);
    assert_eq!(r.already_filed, 0);
    assert!(from.exists(), "source must be untouched");
    assert!(!to.exists(), "nothing must be written for an unresolvable collision");
}

/// Content already claimed under a SUFFIXED name must not be filed again.
/// Three same-named cards bound for one directory: the first takes `c.png`,
/// the second differs and takes `c (2).png`, and the third is byte-identical
/// to the second. Comparing it only against the first (which it differs from)
/// sends it to `c (3).png` — two identical files, manufactured by the very
/// rule that exists to prevent duplicates. Card packs name every file
/// `card.png` and identical cards across packs are the norm, so this chain is
/// the common case.
#[test]
fn identical_content_already_claimed_under_a_suffix_is_not_filed_a_second_time() {
    let root = fresh("suffix_same_content");
    let x = kk_female();
    let y = card("【KoiKatuChara】", 1, "別", "人", Some(2));
    assert_ne!(x, y, "the fixture must actually differ");
    let a = put(&root, "A/c.png", &x);
    let b = put(&root, "B/c.png", &y);
    let c = put(&root, "C/c.png", &y); // byte-identical to B

    let p = organize::plan(&root, true, None);
    assert_eq!(p.moves.len(), 3, "{p:?}");
    assert!(matches!(p.moves[0].collision, organize::Collision::None), "{p:?}");
    assert!(matches!(p.moves[1].collision, organize::Collision::Renamed), "{p:?}");
    assert!(
        matches!(p.moves[2].collision, organize::Collision::AlreadyFiled),
        "content already claimed under a suffix must read as AlreadyFiled: {p:?}"
    );
    assert_eq!(p.moves[2].to, p.moves[1].to, "it is the same content, so the same destination");

    let r = organize::apply(&p);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!((r.moved, r.renamed, r.already_filed), (1, 1, 1));
    assert_eq!(fs::read(root.join("Koikatu/Female/c.png")).unwrap(), x);
    assert_eq!(fs::read(root.join("Koikatu/Female/c (2).png")).unwrap(), y);
    assert!(
        !root.join("Koikatu/Female/c (3).png").exists(),
        "identical bytes must not be filed twice under two suffixes"
    );
    assert!(!a.exists() && !b.exists(), "both distinct cards moved");
    assert!(c.exists(), "the already-filed duplicate's source is untouched");
}

/// A destination directory that cannot be LISTED (a dropped network share, an
/// ACL that denies list while permitting write — reproduced portably here by a
/// file sitting where the directory should be) leaves the collision status
/// unknown. Reporting it as "no collision" plans a move whose `fs::rename`
/// silently replaces a card that was there all along, with an empty `errors`
/// and a clean `{"moved": N}`. It must be reported and nothing may be planned.
#[test]
fn an_unlistable_destination_directory_is_reported_not_treated_as_free() {
    let root = fresh("dest_unlistable");
    let src = put(&root, "in/c.png", &kk_female());
    // Where `Koikatu/Female/` should be, put a FILE: read_dir then fails with
    // something that is emphatically not NotFound.
    let blocker = put(&root, "Koikatu/Female", b"not a directory");

    let p = organize::plan(&root, true, None);
    assert!(p.moves.is_empty(), "an unknown collision status must plan no move: {p:?}");
    assert_eq!(p.unreadable.len(), 1, "{p:?}");
    assert_eq!(p.unreadable[0].path, src);
    assert!(
        p.unreadable[0].reason.contains("could not be listed"),
        "{}",
        p.unreadable[0].reason
    );

    let r = organize::apply(&p);
    assert_eq!(r.moved, 0);
    assert!(r.errors.is_empty(), "nothing was planned, so nothing errors: {:?}", r.errors);
    assert!(src.exists(), "the source card must be untouched");
    assert_eq!(fs::read(&blocker).unwrap(), b"not a directory", "the blocker must be untouched");
}

/// The backstop for every residual plan/reality mismatch, including genuine
/// TOCTOU: `fs::rename` replaces the destination silently on both Windows and
/// POSIX, so `apply` must verify the destination is still free instead of
/// trusting the plan. Without the guard this test destroys `victim` and
/// reports a clean move.
#[test]
fn apply_refuses_to_move_onto_a_destination_that_exists_despite_the_plan() {
    for (name, collision) in [
        ("apply_guard_none", organize::Collision::None),
        ("apply_guard_renamed", organize::Collision::Renamed),
    ] {
        let root = fresh(name);
        let from = put(&root, "in/c.png", &kk_female());
        let victim_bytes = card("【KoiKatuChara】", 1, "別", "人", Some(2));
        let to = put(&root, "Koikatu/Female/c.png", &victim_bytes);

        let p = organize::Plan {
            moves: vec![organize::Planned { from: from.clone(), to: to.clone(), collision }],
            ..Default::default()
        };
        let r = organize::apply(&p);

        assert_eq!(r.errors.len(), 1, "{collision:?}: {:?}", r.errors);
        assert_eq!(r.moved, 0, "{collision:?}");
        assert_eq!(r.renamed, 0, "{collision:?}");
        assert_eq!(
            fs::read(&to).unwrap(),
            victim_bytes,
            "{collision:?}: the card already at the destination must survive"
        );
        assert!(from.exists(), "{collision:?}: the source must be untouched");
    }
}

/// A KKS card whose personality the target KK install cannot voice converts
/// cleanly and then loads with no voice at all — silently. Surface it before
/// the conversion step, never after.
#[test]
fn a_kks_card_with_an_unsupported_personality_is_flagged() {
    let root = fresh("voice");
    // Fake a game install exposing personalities 0..=2 only.
    let game = root.join("game");
    for n in 0..=2 {
        fs::create_dir_all(game.join(format!("abdata/sound/data/pcm/c{n:02}"))).unwrap();
    }
    let support = organize::voice_support(&game);
    assert_eq!(support.ids.len(), 3, "derived from the install, not hardcoded");

    put(&root, "in/ok.png", &card("【KoiKatuCharaSun】", 1, "a", "b", Some(2)));
    let bad = put(&root, "in/mute.png", &card("【KoiKatuCharaSun】", 1, "c", "d", Some(77)));

    let p = organize::plan(&root, true, Some(&support));
    assert_eq!(p.voice_incompatible.len(), 1, "{p:?}");
    assert_eq!(p.voice_incompatible[0].path, bad);
    assert_eq!(p.voice_incompatible[0].personality, 77);
    // Flagging must not stop the card being classified.
    assert_eq!(p.moves.len(), 2, "both KKS cards still get filed");
}

/// KK cards are the conversion target, so their personalities are not checked.
#[test]
fn a_kk_card_is_never_voice_flagged() {
    let root = fresh("voice_kk");
    let game = root.join("game");
    fs::create_dir_all(game.join("abdata/sound/data/pcm/c00")).unwrap();
    let support = organize::voice_support(&game);
    put(&root, "in/kk.png", &card("【KoiKatuChara】", 1, "a", "b", Some(77)));
    let p = organize::plan(&root, true, Some(&support));
    assert!(p.voice_incompatible.is_empty(), "{p:?}");
}

/// Without a game root there is nothing to check against — say so rather than
/// guessing a supported set.
#[test]
fn without_a_game_root_no_voice_claim_is_made() {
    let root = fresh("voice_none");
    put(&root, "in/kks.png", &card("【KoiKatuCharaSun】", 1, "a", "b", Some(77)));
    let p = organize::plan(&root, true, None);
    assert!(p.voice_incompatible.is_empty());
}

/// The user's actual conversion step operates on cards already filed in the
/// Sunshine destination folder from an earlier pass. If those are never
/// voice-checked, "surface it before conversion" is not delivered for a
/// re-run — exactly the population the check exists to protect. Flagging
/// must not change where the card goes: it stays in `skipped`, never gains
/// a `moves` entry.
#[test]
fn an_already_filed_kks_card_with_an_unsupported_personality_is_still_flagged() {
    let root = fresh("voice_filed");
    let game = root.join("game");
    fs::create_dir_all(game.join("abdata/sound/data/pcm/c00")).unwrap();
    let support = organize::voice_support(&game);

    let filed = put(
        &root,
        "KoikatsuSunshine/Female/c.png",
        &card("【KoiKatuCharaSun】", 1, "a", "b", Some(77)),
    );

    let p = organize::plan(&root, true, Some(&support));
    assert_eq!(p.voice_incompatible.len(), 1, "{p:?}");
    assert_eq!(p.voice_incompatible[0].path, filed);
    assert_eq!(p.voice_incompatible[0].personality, 77);
    assert_eq!(p.skipped, vec![filed], "still just skipped, never an error");
    assert!(p.moves.is_empty(), "an already-filed card must not move: {p:?}");
}

/// A read failure on an already-filed file must not turn a clean `skipped`
/// into an error: it is reported the same way any other unreadable file is,
/// and the skip itself still happens.
#[test]
fn an_already_filed_unreadable_png_stays_skipped_and_is_also_reported() {
    let root = fresh("voice_filed_bad");
    let game = root.join("game");
    fs::create_dir_all(game.join("abdata/sound/data/pcm/c00")).unwrap();
    let support = organize::voice_support(&game);

    let filed = put(
        &root,
        "KoikatsuSunshine/Female/broken.png",
        b"\x89PNG\r\n\x1a\nnot really a png body",
    );

    let p = organize::plan(&root, true, Some(&support));
    assert_eq!(p.skipped, vec![filed.clone()], "{p:?}");
    assert!(p.moves.is_empty());
    assert_eq!(p.unreadable.len(), 1, "{p:?}");
    assert_eq!(p.unreadable[0].path, filed);
}

/// A scan that could not read the `pcm` directory at all (a mistyped game
/// root, the realistic failure) must not be indistinguishable from a
/// genuinely empty install: it must make no voice claim whatsoever, the
/// same treatment as no game root being supplied, rather than flagging
/// every personality as unsupported.
#[test]
fn a_failed_scan_makes_no_voice_claim() {
    let root = fresh("voice_scan_failed");
    let game = root.join("does_not_exist");
    let support = organize::voice_support(&game);
    assert!(!support.ok, "a missing pcm directory is a failed scan, not an empty install");
    assert!(support.ids.is_empty());

    put(&root, "in/kks.png", &card("【KoiKatuCharaSun】", 1, "a", "b", Some(77)));
    let p = organize::plan(&root, true, Some(&support));
    assert!(p.voice_incompatible.is_empty(), "a failed scan must not flag any card: {p:?}");
}

/// A `pcm` directory that opens cleanly but genuinely contains no personality
/// subfolders is a real (if unusual) empty install, not a failed scan: `ok`
/// must be true, distinguishing it from the failed-scan case above, and its
/// legitimately empty supported set really does mean every personality is
/// unsupported.
#[test]
fn a_genuinely_empty_install_is_distinguishable_from_a_failed_scan() {
    let root = fresh("voice_scan_empty");
    let game = root.join("game");
    fs::create_dir_all(game.join("abdata/sound/data/pcm")).unwrap();
    let support = organize::voice_support(&game);
    assert!(support.ok, "the directory was read successfully, just empty");
    assert!(support.ids.is_empty());

    put(&root, "in/kks.png", &card("【KoiKatuCharaSun】", 1, "a", "b", Some(77)));
    let p = organize::plan(&root, true, Some(&support));
    assert_eq!(
        p.voice_incompatible.len(), 1,
        "a genuinely empty install really does not support this personality: {p:?}"
    );
}

/// A stray FILE named like a personality folder (`c5`) must not be counted
/// as support for it — only directories are personalities under `pcm`.
#[test]
fn a_file_named_like_a_personality_folder_is_not_counted() {
    let root = fresh("voice_file_not_dir");
    let game = root.join("game");
    let pcm = game.join("abdata/sound/data/pcm");
    fs::create_dir_all(&pcm).unwrap();
    fs::write(pcm.join("c05"), b"not a directory").unwrap();
    let support = organize::voice_support(&game);
    assert!(support.ids.is_empty(), "a file, not a directory, must not count: {:?}", support.ids);
}
