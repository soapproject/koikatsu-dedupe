//! Parser tests for card.rs, driven by synthesised cards from tests/common.

mod common;

mod tests {
    use crate::common::fixture::*;
    use app_lib::card::*;

    fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("kdedupe_card_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn reads_a_kk_female_character_card() {
        let p = write("kk_f.png", &card("【KoiKatuChara】", 1, "東山", "涼子", Some(19)));
        let m = read_card(&p).expect("should parse");
        assert_eq!(m.game.folder(), "Koikatu");
        assert!(matches!(m.card_type, CardType::Character));
        assert_eq!(m.sex.folder(), "Female");
        assert_eq!(m.lastname, "東山");
        assert_eq!(m.firstname, "涼子");
        assert_eq!(m.personality, Some(19));
    }

    #[test]
    fn reads_a_kks_card_and_keeps_the_games_apart() {
        let p = write("kks_f.png", &card("【KoiKatuCharaSun】", 1, "", "泽近爱理", Some(27)));
        let m = read_card(&p).expect("should parse");
        assert_eq!(m.game.folder(), "KoikatsuSunshine");
        assert_eq!(m.personality, Some(27));
    }

    #[test]
    fn a_coordinate_card_needs_no_parameter_block() {
        let p = write("kk_coord.png", &card("【KoiKatuClothes】", 0, "x", "y", None));
        let m = read_card(&p).expect("should parse");
        assert!(matches!(m.card_type, CardType::Coordinate));
        assert_eq!(m.card_type.folder(), "Coordinate");
    }

    #[test]
    fn an_unknown_marker_is_reported_never_guessed() {
        let p = write("scene.png", &card("【KStudio】", 1, "a", "b", Some(1)));
        match read_card(&p) {
            Err(CardError::Unrecognized(m)) => assert!(m.contains("KStudio")),
            other => panic!("expected Unrecognized, got {other:?}"),
        }
    }

    #[test]
    fn a_plain_png_is_not_a_card() {
        let p = write("plain.png", &png_prefix());
        assert!(matches!(read_card(&p), Err(CardError::NotCard) | Err(CardError::Malformed(_))));
    }

    /// The fixture's SHAPE is load-bearing, so it is asserted rather than
    /// assumed. A card whose face image is empty never runs card.rs's
    /// `if face > 0 { skip }`, and a card whose only block sits at `pos = 0`
    /// never runs `base + bi.pos` with a real offset — both lines could be
    /// wrong with the whole suite green, while every real card fell into
    /// `unreadable`. Real cards list `KKEx` first in the table and place
    /// `Parameter` well past the start; this pins the fixture to that shape.
    #[test]
    fn the_fixture_has_a_real_cards_shape_so_the_risky_parser_lines_run() {
        let p = write("kk_shape.png", &card("【KoiKatuChara】", 1, "東山", "涼子", Some(19)));
        let m = read_card(&p).expect("a card with a face image and three blocks must parse");
        assert_eq!(m.blocks.len(), 3, "several blocks, not one: {:?}", m.blocks);
        assert_eq!(m.blocks[0].name, "KKEx", "the table must not list Parameter first");
        let param = m
            .blocks
            .iter()
            .find(|b| b.name == "Parameter")
            .expect("Parameter must be findable among the others");
        assert!(param.pos > 0, "Parameter at pos 0 leaves `base + pos` unexercised");
        // ...and the Parameter block still decodes correctly from that offset.
        assert_eq!(m.sex.folder(), "Female");
        assert_eq!(m.lastname, "東山");
        assert_eq!(m.personality, Some(19));
    }

    /// The synthesised cards above are the whole suite's input, so nothing
    /// proves the parser agrees with cards Koikatsu actually wrote. These
    /// fixtures are local-only (real cards are copyrighted, so they are not
    /// shipped); the test skips itself per file when they are absent, the
    /// same pattern round.rs uses. Values are pinned from the real files:
    /// the set deliberately spans a card with no `KKEx` block, one with
    /// `KKEx` listed first, and a `KoiKatuCharaSP` marker.
    #[test]
    fn real_cards_from_testdata_parse_with_the_expected_metadata() {
        let td = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("testdata");
        let expected: &[(&str, &str, &str, i64)] = &[
            // file, game folder, sex folder, personality
            ("KK_387575.png", "Koikatu", "Female", 6),
            ("KK_387576.png", "Koikatu", "Female", 19), // no KKEx block
            ("KKconv_KK_341944.png", "Koikatu", "Female", 18), // KKEx listed first
            ("100001890_12.png", "Koikatu", "Female", 6), // KoiKatuCharaSP marker
            ("Koikatu_F_20180209182122015.png", "Koikatu", "Female", 8),
        ];
        let mut checked = 0;
        for (name, game, sex, personality) in expected {
            let p = td.join(name);
            if !p.exists() {
                continue; // fixtures are local-only (real cards, not shipped)
            }
            let m = read_card(&p).unwrap_or_else(|e| panic!("{name} must parse: {}", e.reason()));
            assert_eq!(m.game.folder(), *game, "{name} game");
            assert_eq!(m.card_type.folder(), "Character", "{name} card type");
            assert_eq!(m.sex.folder(), *sex, "{name} sex");
            assert_eq!(m.personality, Some(*personality), "{name} personality");
            assert!(
                m.blocks.iter().any(|b| b.name == "Parameter"),
                "{name} must expose a Parameter block: {:?}",
                m.blocks
            );
            checked += 1;
        }
        if checked > 0 {
            // At least one real card must list KKEx before Parameter, which is
            // the layout the synthesised fixture mimics.
            let kkex = td.join("KKconv_KK_341944.png");
            if kkex.exists() {
                let m = read_card(&kkex).expect("parse");
                assert_eq!(m.blocks[0].name, "KKEx");
                let param = m.blocks.iter().find(|b| b.name == "Parameter").unwrap();
                assert!(param.pos > 0, "a real Parameter block never sits at pos 0");
            }
        }
    }

    #[test]
    fn a_non_utf8_name_does_not_panic() {
        // Build a card, then corrupt the firstname bytes to invalid UTF-8.
        let mut bytes = card("【KoiKatuChara】", 1, "AA", "BB", Some(3));
        let idx = bytes.windows(2).rposition(|w| w == b"BB").unwrap();
        bytes[idx] = 0xFF;
        let p = write("bad_utf8.png", &bytes);
        let m = read_card(&p).expect("must not panic or fail");
        assert!(m.firstname.contains('\u{FFFD}'), "expected lossy replacement, got {:?}", m.firstname);
    }
}
