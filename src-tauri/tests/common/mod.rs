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

        let table = mp_map(&[(
            "lstInfo",
            mp_arr(&[mp_map(&[
                ("name", mp_str("Parameter")),
                ("version", mp_str("0.0.5")),
                ("pos", mp_int(0)),
                ("size", mp_int(param_bytes.len() as i64)),
            ])]),
        )]);

        let mut v = png_prefix();
        v.extend_from_slice(&100i32.to_le_bytes()); // ProductNo
        v.extend(net_str(marker));
        v.extend(net_str("0.0.0")); // load version
        v.extend_from_slice(&0i32.to_le_bytes()); // face png length
        v.extend_from_slice(&(table.len() as i32).to_le_bytes());
        v.extend_from_slice(&table);
        v.extend_from_slice(&(param_bytes.len() as i64).to_le_bytes()); // total
        v.extend_from_slice(&param_bytes);
        v
    }
}
