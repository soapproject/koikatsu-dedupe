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
