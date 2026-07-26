//! Synthesises byte-exact Koikatsu cards so the test suite needs no
//! local-only fixtures. Lives under tests/common/ — not a test target
//! itself — so no test-only code ends up in the shipped library.
#![allow(dead_code)] // each test crate uses a different subset

pub mod fixture {
    /// MessagePack fixstr / fixmap / fixarray writers (test-only encoder).
    pub fn mp_str(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        assert!(b.len() < 32, "fixture strings stay in fixstr range");
        let mut v = vec![0xA0 | b.len() as u8];
        v.extend_from_slice(b);
        v
    }
    pub fn mp_int(i: i64) -> Vec<u8> {
        assert!((0..128).contains(&i), "fixture ints stay in positive fixint range");
        vec![i as u8]
    }
    /// Width-adaptive writer for block-table offsets and sizes — the one
    /// fixture field whose value is derived rather than chosen, since it grows
    /// with the card's own contents. An `mp_int`-style "must be < 128"
    /// assertion would turn a longer character name into a fixture panic.
    ///
    /// Coverage, stated honestly: with today's fixture every offset and size
    /// fits in a positive fixint (largest offset 82, largest size 66), so the
    /// `0xCD` / `0xCE` branches are NOT reached and this helper exercises no
    /// wide-integer path in the decoder — `msgpack.rs`'s own `wide_types_decode`
    /// is what covers those. The branches stay so a future fixture that does
    /// outgrow the range keeps working rather than panicking.
    pub fn mp_uint(n: u64) -> Vec<u8> {
        if n < 128 {
            vec![n as u8]
        } else if n <= u16::MAX as u64 {
            let mut v = vec![0xCD];
            v.extend_from_slice(&(n as u16).to_be_bytes());
            v
        } else {
            let mut v = vec![0xCE];
            v.extend_from_slice(&(n as u32).to_be_bytes());
            v
        }
    }
    pub fn mp_map(pairs: &[(&str, Vec<u8>)]) -> Vec<u8> {
        assert!(pairs.len() < 16);
        let mut v = vec![0x80 | pairs.len() as u8];
        for (k, val) in pairs {
            v.extend(mp_str(k));
            v.extend_from_slice(val);
        }
        v
    }
    pub fn mp_arr(items: &[Vec<u8>]) -> Vec<u8> {
        assert!(items.len() < 16);
        let mut v = vec![0x90 | items.len() as u8];
        for i in items {
            v.extend_from_slice(i);
        }
        v
    }

    /// .NET BinaryReader string: 7-bit encoded length prefix + UTF-8.
    pub fn net_str(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        assert!(b.len() < 128, "fixture markers are short");
        let mut v = vec![b.len() as u8];
        v.extend_from_slice(b);
        v
    }

    /// A minimal but structurally real PNG: signature + IHDR + IEND.
    pub fn png_prefix() -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&[0u8; 13]);
        v.extend_from_slice(&[0u8; 4]); // crc (never verified by the parser)
        v.extend_from_slice(&0u32.to_be_bytes());
        v.extend_from_slice(b"IEND");
        v.extend_from_slice(&[0u8; 4]); // crc
        v
    }

    /// Build a whole card. `personality` of None omits the key entirely.
    ///
    /// Shaped like a REAL card, not like the minimum the parser accepts, so
    /// the two riskiest lines in card.rs are actually executed:
    ///
    /// * a **non-empty face PNG**, so the `if face > 0 { skip }` branch runs.
    ///   A card whose face length is 0 leaves that skip untaken, and every
    ///   real card would then land in `unreadable` while the suite stayed
    ///   green.
    /// * **three blocks**, with `Parameter` neither first in the table nor at
    ///   `pos = 0`, so `base + bi.pos` is exercised with a non-zero `pos` and
    ///   the `find(name == "Parameter")` has to pick it out of several. Real
    ///   cards list `KKEx` FIRST in the table but place it LAST in the data
    ///   area; the fixture mimics that, so table order and positional order
    ///   deliberately disagree.
    pub fn card(marker: &str, sex: i64, lastname: &str, firstname: &str, personality: Option<i64>) -> Vec<u8> {
        let mut param = vec![
            ("version", mp_str("0.0.5")),
            ("sex", mp_int(sex)),
            ("lastname", mp_str(lastname)),
            ("firstname", mp_str(firstname)),
        ];
        if let Some(p) = personality {
            param.push(("personality", mp_int(p)));
        }
        let param_bytes = mp_map(&param);
        // Two filler blocks. Contents are never decoded (card.rs reads only
        // Parameter) — they exist to give Parameter a non-zero position and
        // company in the table.
        let custom_bytes = mp_map(&[("clothes", mp_int(1)), ("hair", mp_int(2))]);
        let kkex_bytes = mp_map(&[("info", mp_str("kkex"))]);

        // Data-area layout: Custom, Parameter, KKEx.
        let custom_pos = 0u64;
        let param_pos = custom_bytes.len() as u64;
        let kkex_pos = param_pos + param_bytes.len() as u64;
        let entry = |name: &str, pos: u64, size: usize| {
            mp_map(&[
                ("name", mp_str(name)),
                ("version", mp_str("0.0.5")),
                ("pos", mp_uint(pos)),
                ("size", mp_uint(size as u64)),
            ])
        };
        // Table order: KKEx first (as real cards list it), Parameter last.
        let table = mp_map(&[(
            "lstInfo",
            mp_arr(&[
                entry("KKEx", kkex_pos, kkex_bytes.len()),
                entry("Custom", custom_pos, custom_bytes.len()),
                entry("Parameter", param_pos, param_bytes.len()),
            ]),
        )]);
        // The face image a card carries between the load version and the
        // block table. Any bytes will do — the parser only skips them.
        let face = png_prefix();
        let total = custom_bytes.len() + param_bytes.len() + kkex_bytes.len();

        let mut v = png_prefix();
        v.extend_from_slice(&100i32.to_le_bytes()); // ProductNo
        v.extend(net_str(marker));
        v.extend(net_str("0.0.0")); // load version
        v.extend_from_slice(&(face.len() as i32).to_le_bytes()); // face png length
        v.extend_from_slice(&face);
        v.extend_from_slice(&(table.len() as i32).to_le_bytes());
        v.extend_from_slice(&table);
        v.extend_from_slice(&(total as i64).to_le_bytes()); // total
        v.extend_from_slice(&custom_bytes);
        v.extend_from_slice(&param_bytes);
        v.extend_from_slice(&kkex_bytes);
        v
    }
}
