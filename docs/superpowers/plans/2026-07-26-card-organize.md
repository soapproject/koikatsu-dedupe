# 卡片整理模組（organize）實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `koikatsu-dedupe` 內做出一個可獨立執行的卡片整理模組（後端 + `kdedupe organize` CLI），取代 `koikatsu-hamster.exe`，並修掉它靜默漏卡的缺陷。

**Architecture:** 三個新模組疊起來：`msgpack.rs`（最小 MessagePack 解碼器，零新增相依）→ `card.rs`（結構化卡片 metadata，接在既有 `core::png_char_block()` 之後）→ `organize.rs`（分類規劃與套用）。`core.rs` 不動，維持純去重。CLI 加一個 `organize` 子命令。

**Tech Stack:** Rust 2021（rust-version 1.77.2）、rusqlite、serde_json、既有手寫 CLI 參數解析。**不新增任何 crate。**

對應規格：`docs/superpowers/specs/2026-07-26-card-organize-design.md`

## Global Constraints

- **不新增任何 cargo 相依。** 本專案相依刻意極簡（`twox-hash`/`rusqlite`/`trash`/serde/tauri）；CLI 參數解析與 PNG chunk 走訪都是手寫的。
- **不修改 SQLite schema。** `core.rs` 的 `open_db()` 保持原樣。
- **不修改 `core.rs` 的任何既有函式。** 只讀取 `core::png_char_block()`。
- 卡片姓名可能不是合法 UTF-8 → 一律 lossy 解碼，**絕不 panic**，絕不當識別鍵。
- marker 不在對應表 → `Unrecognized`，回報，不搬動，**不猜測**。
- 任何解析失敗 → `Unreadable { reason }`，回報，不搬動。
- 目的地資料夾名集合固定為：`Koikatu`、`KoikatsuSunshine`、`HoneyCome`、`SVC`、`Aicomi`。
- 檔名比對一律 case-insensitive（Windows 視 `A.png` 與 `a.png` 為同一檔）。
- 新模組在 `lib.rs` 用 `pub mod`，在 `cli.rs` 用既有的 `#[path = "..."] mod` 風格宣告。
- 測試一律**自行合成卡片位元組**，不依賴 `testdata/` 本機夾具，這樣 8 條回歸測試在任何機器上都跑得到。

### 對規格的一處刻意收斂（實作時照本計畫，spec 已同步修正）

規格原寫「支援性格集合 = 安裝目錄 `abdata\sound\data\pcm\c<N>` ∪ 提供該路徑的 zipmod」。
**本計畫只實作安裝目錄那半。** 讀 zipmod 需要 zip crate，違反零新增相依；而實測基礎遊戲加
20,502 個模組**沒有任何模組新增性格**（集合就是連續的 0–38）。報告會明講集合來源與此限制。
真的出現性格模組時再加，屆時 zip 相依才有實據。

---

## File Structure

| 檔案 | 職責 |
|---|---|
| `src-tauri/src/msgpack.rs` **(建立)** | 最小 MessagePack 解碼器：`Value` 列舉 + `Reader`。只解碼，不編碼。 |
| `src-tauri/src/card.rs` **(建立)** | 卡片結構化 metadata：.NET `BinaryReader` 字串、區塊表、marker 對應、`Parameter` 欄位擷取。 |
| `src-tauri/src/organize.rs` **(建立)** | 分類決策（含排除規則）、性格相容性、規劃與套用、撞名處理。 |
| `src-tauri/src/lib.rs` **(修改 :1)** | 加 `pub mod msgpack; pub mod card; pub mod organize;` |
| `src-tauri/src/cli.rs` **(修改)** | 加三個 `#[path]` mod 宣告、`organize` 子命令、USAGE、`describe` 條目、`plan` 進 `BOOL_FLAGS`。 |
| `src-tauri/tests/organize.rs` **(建立)** | 8 條回歸測試，用自建合成卡片，完全 hermetic。 |
| `src-tauri/tests/cli.rs` **(修改)** | 加 `organize` 的 CLI 端到端測試（dry-run 不動檔案）。 |

---

## Task 1: 最小 MessagePack 解碼器

**Files:**
- Create: `src-tauri/src/msgpack.rs`
- Modify: `src-tauri/src/lib.rs:1`

**Interfaces:**
- Consumes: 無（本模組不依賴專案任何既有程式碼）
- Produces:
  - `pub enum Value { Nil, Bool(bool), Int(i64), UInt(u64), F32(f32), F64(f64), Str(String), Bin(Vec<u8>), Array(Vec<Value>), Map(Vec<(Value, Value)>) }`
  - `impl Value { pub fn get(&self, key: &str) -> Option<&Value>; pub fn as_str(&self) -> Option<&str>; pub fn as_i64(&self) -> Option<i64>; pub fn as_array(&self) -> Option<&[Value]> }`
  - `pub struct Reader<'a>`，`Reader::new(buf: &'a [u8]) -> Reader<'a>`、`Reader::pos(&self) -> usize`、`Reader::read(&mut self) -> Result<Value, String>`
  - `pub fn decode(buf: &[u8]) -> Result<Value, String>`

- [ ] **Step 1: 寫失敗測試**

在 `src-tauri/src/msgpack.rs` 建立檔案，內容只放測試（實作下一步才寫）：

```rust
//! Minimal MessagePack decoder. Decode-only, zero dependencies.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_string_keyed_map_like_a_card_parameter_block() {
        // fixmap(3) { "version": "0.0.5", "sex": 1, "personality": 19 }
        let buf = [
            0x83, // fixmap, 3 pairs
            0xA7, b'v', b'e', b'r', b's', b'i', b'o', b'n', // fixstr(7) "version"
            0xA5, b'0', b'.', b'0', b'.', b'5', // fixstr(5) "0.0.5"
            0xA3, b's', b'e', b'x', // fixstr(3) "sex"
            0x01, // positive fixint 1
            0xAB, b'p', b'e', b'r', b's', b'o', b'n', b'a', b'l', b'i', b't', b'y',
            0x13, // positive fixint 19
        ];
        let v = decode(&buf).expect("decode");
        assert_eq!(v.get("version").and_then(|x| x.as_str()), Some("0.0.5"));
        assert_eq!(v.get("sex").and_then(|x| x.as_i64()), Some(1));
        assert_eq!(v.get("personality").and_then(|x| x.as_i64()), Some(19));
    }

    #[test]
    fn decodes_nested_array_of_maps_like_a_block_table() {
        // fixmap(1) { "lstInfo": fixarray(1) [ fixmap(2) { "name":"Parameter", "size":5 } ] }
        let buf = [
            0x81, 0xA7, b'l', b's', b't', b'I', b'n', b'f', b'o', 0x91, 0x82, 0xA4, b'n', b'a',
            b'm', b'e', 0xA9, b'P', b'a', b'r', b'a', b'm', b'e', b't', b'e', b'r', 0xA4, b's',
            b'i', b'z', b'e', 0x05,
        ];
        let v = decode(&buf).expect("decode");
        let list = v.get("lstInfo").and_then(|x| x.as_array()).expect("lstInfo");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].get("name").and_then(|x| x.as_str()), Some("Parameter"));
        assert_eq!(list[0].get("size").and_then(|x| x.as_i64()), Some(5));
    }

    #[test]
    fn a_non_utf8_string_decodes_lossily_instead_of_failing() {
        // fixstr(2) with an invalid UTF-8 byte: card names are not guaranteed UTF-8.
        let buf = [0xA2, 0xFF, b'a'];
        let v = decode(&buf).expect("must not error on invalid UTF-8");
        assert_eq!(v.as_str(), Some("\u{FFFD}a"));
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        let buf = [0xA5, b'0', b'.']; // fixstr(5) but only 2 bytes follow
        assert!(decode(&buf).is_err());
    }

    #[test]
    fn wide_types_decode() {
        // uint32 0x0001_0000, int8 -3, str8 "hi", bin8 [1,2]
        assert_eq!(decode(&[0xCE, 0x00, 0x01, 0x00, 0x00]).unwrap().as_i64(), Some(65536));
        assert_eq!(decode(&[0xD0, 0xFD]).unwrap().as_i64(), Some(-3));
        assert_eq!(decode(&[0xD9, 0x02, b'h', b'i']).unwrap().as_str(), Some("hi"));
        assert!(matches!(decode(&[0xC4, 0x02, 0x01, 0x02]).unwrap(), Value::Bin(b) if b == vec![1, 2]));
    }
}
```

同時修改 `src-tauri/src/lib.rs` 第 1 行，由：

```rust
pub mod core;
```

改為：

```rust
pub mod core;
pub mod msgpack;
```

（`card.rs` 與 `organize.rs` 分別在 Task 2、Task 3 建立時才加自己的那行；提早宣告會編譯失敗。）

- [ ] **Step 2: 跑測試確認失敗**

Run: `cd src-tauri && cargo test --lib msgpack`
Expected: 編譯失敗，`cannot find function decode in this scope`（尚未實作）。

- [ ] **Step 3: 寫最小實作**

在 `src-tauri/src/msgpack.rs` 的 `#[cfg(test)]` 區塊**之前**插入：

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    UInt(u64),
    F32(f32),
    F64(f64),
    Str(String),
    Bin(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

impl Value {
    /// Card blocks are string-keyed maps; look a key up by name.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(pairs) => pairs
                .iter()
                .find(|(k, _)| k.as_str() == Some(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::UInt(u) => i64::try_from(*u).ok(),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(v) => Some(v),
            _ => None,
        }
    }
}

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }
    /// Byte offset of the next unread byte. Lets a caller locate a decoded
    /// field inside the original buffer without re-encoding anything.
    pub fn pos(&self) -> usize {
        self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or("length overflow")?;
        let s = self.buf.get(self.pos..end).ok_or("truncated msgpack")?;
        self.pos = end;
        Ok(s)
    }
    fn u8v(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn be(&mut self, n: usize) -> Result<u64, String> {
        let s = self.take(n)?;
        Ok(s.iter().fold(0u64, |a, b| (a << 8) | *b as u64))
    }
    /// Card names are not guaranteed valid UTF-8 — decode lossily, never fail.
    fn string(&mut self, n: usize) -> Result<Value, String> {
        Ok(Value::Str(String::from_utf8_lossy(self.take(n)?).into_owned()))
    }
    fn array(&mut self, n: usize) -> Result<Value, String> {
        let mut v = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            v.push(self.read()?);
        }
        Ok(Value::Array(v))
    }
    fn map(&mut self, n: usize) -> Result<Value, String> {
        let mut v = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            let k = self.read()?;
            let val = self.read()?;
            v.push((k, val));
        }
        Ok(Value::Map(v))
    }

    pub fn read(&mut self) -> Result<Value, String> {
        let c = self.u8v()?;
        Ok(match c {
            0x00..=0x7F => Value::Int(c as i64),
            0xE0..=0xFF => Value::Int(c as i8 as i64),
            0x80..=0x8F => self.map((c & 0x0F) as usize)?,
            0x90..=0x9F => self.array((c & 0x0F) as usize)?,
            0xA0..=0xBF => self.string((c & 0x1F) as usize)?,
            0xC0 => Value::Nil,
            0xC2 => Value::Bool(false),
            0xC3 => Value::Bool(true),
            0xC4 => { let n = self.be(1)? as usize; Value::Bin(self.take(n)?.to_vec()) }
            0xC5 => { let n = self.be(2)? as usize; Value::Bin(self.take(n)?.to_vec()) }
            0xC6 => { let n = self.be(4)? as usize; Value::Bin(self.take(n)?.to_vec()) }
            0xCA => Value::F32(f32::from_bits(self.be(4)? as u32)),
            0xCB => Value::F64(f64::from_bits(self.be(8)?)),
            0xCC => Value::UInt(self.be(1)?),
            0xCD => Value::UInt(self.be(2)?),
            0xCE => Value::UInt(self.be(4)?),
            0xCF => Value::UInt(self.be(8)?),
            0xD0 => Value::Int(self.be(1)? as u8 as i8 as i64),
            0xD1 => Value::Int(self.be(2)? as u16 as i16 as i64),
            0xD2 => Value::Int(self.be(4)? as u32 as i32 as i64),
            0xD3 => Value::Int(self.be(8)? as i64),
            0xD9 => { let n = self.be(1)? as usize; self.string(n)? }
            0xDA => { let n = self.be(2)? as usize; self.string(n)? }
            0xDB => { let n = self.be(4)? as usize; self.string(n)? }
            0xDC => { let n = self.be(2)? as usize; self.array(n)? }
            0xDD => { let n = self.be(4)? as usize; self.array(n)? }
            0xDE => { let n = self.be(2)? as usize; self.map(n)? }
            0xDF => { let n = self.be(4)? as usize; self.map(n)? }
            other => return Err(format!("unsupported msgpack byte 0x{other:02X}")),
        })
    }
}

pub fn decode(buf: &[u8]) -> Result<Value, String> {
    Reader::new(buf).read()
}
```

- [ ] **Step 4: 跑測試確認通過**

Run: `cd src-tauri && cargo test --lib msgpack`
Expected: 5 個測試 PASS，無警告。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/msgpack.rs src-tauri/src/lib.rs
git commit -m "feat(msgpack): minimal decode-only MessagePack reader

Card blocks are string-keyed MessagePack maps. Decode-only and
dependency-free: the conversion work this unblocks patches bytes in place
rather than re-serializing, so an encoder would only add a
value-normalization risk. Strings decode lossily because card names are
not guaranteed valid UTF-8."
```

---

## Task 2: 結構化卡片 metadata

**Files:**
- Create: `src-tauri/src/card.rs`
- Modify: `src-tauri/src/lib.rs`（加 `pub mod card;`）

**Interfaces:**
- Consumes: `crate::core::png_char_block(&Path) -> Option<(u64, u64)>`、`crate::msgpack::{decode, Value}`
- Produces:
  - `pub enum Game { Koikatu, KoikatsuSunshine, HoneyCome, Svc, Aicomi }`，`impl Game { pub fn folder(&self) -> &'static str }`
  - `pub const DEST_FOLDERS: [&str; 5]`
  - `pub enum CardType { Character, Coordinate }`，`impl CardType { pub fn folder(&self) -> &'static str }`
  - `pub enum Sex { Male, Female, Unknown }`，`impl Sex { pub fn folder(&self) -> &'static str }`
  - `pub struct BlockInfo { pub name: String, pub version: String, pub pos: u64, pub size: u64 }`
  - `pub struct CardMeta { pub game: Game, pub card_type: CardType, pub sex: Sex, pub lastname: String, pub firstname: String, pub personality: Option<i64>, pub blocks: Vec<BlockInfo>, pub base: u64 }`
  - `pub enum CardError { NotCard, Unrecognized(String), Malformed(String) }`，`impl CardError { pub fn reason(&self) -> String }`
  - `pub fn read_card(path: &Path) -> Result<CardMeta, CardError>`

- [ ] **Step 1: 寫失敗測試**

建立 `src-tauri/src/card.rs`，先只放測試與合成卡片輔助函式：

```rust
//! Structured Koikatsu card metadata: marker -> (game, card type), and the
//! Parameter block's sex / name / personality. Starts from the offset
//! core::png_char_block() already computes by walking the PNG chunk chain.

#[cfg(test)]
pub mod fixture {
    //! Synthesises byte-exact cards so tests need no local-only fixtures.

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

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;

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
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `cd src-tauri && cargo test --lib card`
Expected: 編譯失敗，`cannot find function read_card` / `cannot find type CardError`。

- [ ] **Step 3: 寫最小實作**

在 `card.rs` 的 `#[cfg(test)]` 區塊**之前**插入：

```rust
use crate::msgpack::{decode, Value};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    Koikatu,
    KoikatsuSunshine,
    HoneyCome,
    Svc,
    Aicomi,
}

impl Game {
    pub fn folder(&self) -> &'static str {
        match self {
            Game::Koikatu => "Koikatu",
            Game::KoikatsuSunshine => "KoikatsuSunshine",
            Game::HoneyCome => "HoneyCome",
            Game::Svc => "SVC",
            Game::Aicomi => "Aicomi",
        }
    }
}

/// Every folder name `organize` may create at the destination root. The
/// exclusion rule compares a path's FIRST segment against exactly these.
pub const DEST_FOLDERS: [&str; 5] = [
    "Koikatu",
    "KoikatsuSunshine",
    "HoneyCome",
    "SVC",
    "Aicomi",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardType {
    Character,
    Coordinate,
}

impl CardType {
    pub fn folder(&self) -> &'static str {
        match self {
            CardType::Character => "Character",
            CardType::Coordinate => "Coordinate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    Male,
    Female,
    Unknown,
}

impl Sex {
    pub fn folder(&self) -> &'static str {
        match self {
            Sex::Male => "Male",
            Sex::Female => "Female",
            Sex::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub name: String,
    pub version: String,
    pub pos: u64,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct CardMeta {
    pub game: Game,
    pub card_type: CardType,
    pub sex: Sex,
    pub lastname: String,
    pub firstname: String,
    pub personality: Option<i64>,
    pub blocks: Vec<BlockInfo>,
    /// Absolute file offset the block table's `pos` values are relative to.
    pub base: u64,
}

#[derive(Debug)]
pub enum CardError {
    /// Not a PNG, or a PNG with no appended Koikatsu block.
    NotCard,
    /// A card of some kind, but its marker is not in the table. Never guessed.
    Unrecognized(String),
    /// Structurally broken past the marker.
    Malformed(String),
}

impl CardError {
    pub fn reason(&self) -> String {
        match self {
            CardError::NotCard => "not a card (no appended Koikatsu block)".into(),
            CardError::Unrecognized(m) => format!("unrecognized marker {m:?}"),
            CardError::Malformed(m) => format!("malformed: {m}"),
        }
    }
}

/// Marker -> (game, card type). Only markers we have seen on a real card are
/// listed; anything else becomes CardError::Unrecognized rather than a guess.
fn classify_marker(marker: &str) -> Option<(Game, CardType)> {
    Some(match marker {
        "【KoiKatuChara】" | "【KoiKatuCharaS】" | "【KoiKatuCharaSP】" => {
            (Game::Koikatu, CardType::Character)
        }
        "【KoiKatuClothes】" => (Game::Koikatu, CardType::Coordinate),
        "【KoiKatuCharaSun】" => (Game::KoikatsuSunshine, CardType::Character),
        "【HCChara】" | "【HCPChara】" => (Game::HoneyCome, CardType::Character),
        "【SVChara】" => (Game::Svc, CardType::Character),
        "【SVClothes】" => (Game::Svc, CardType::Coordinate),
        "【ACChara】" => (Game::Aicomi, CardType::Character),
        "【ACClothes】" => (Game::Aicomi, CardType::Coordinate),
        _ => return None,
    })
}

/// Cursor over the appended block, mirroring .NET BinaryReader primitives.
struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.p.checked_add(n).ok_or("length overflow")?;
        let s = self.b.get(self.p..end).ok_or("truncated card")?;
        self.p = end;
        Ok(s)
    }
    fn i32v(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i64v(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    /// .NET BinaryReader.ReadString: 7-bit encoded length prefix, then UTF-8.
    fn string(&mut self) -> Result<String, String> {
        let mut n: usize = 0;
        let mut shift = 0;
        loop {
            let b = self.take(1)?[0];
            n |= ((b & 0x7F) as usize) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return Err("bad 7-bit length prefix".into());
            }
        }
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

pub fn read_card(path: &Path) -> Result<CardMeta, CardError> {
    let (off, len) = crate::core::png_char_block(path).ok_or(CardError::NotCard)?;
    if len == 0 {
        return Err(CardError::NotCard);
    }
    let mut f = fs::File::open(path).map_err(|e| CardError::Malformed(e.to_string()))?;
    f.seek(SeekFrom::Start(off))
        .map_err(|e| CardError::Malformed(e.to_string()))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| CardError::Malformed(e.to_string()))?;

    let mut c = Cur { b: &buf, p: 0 };
    c.i32v().map_err(CardError::Malformed)?; // ProductNo
    let marker = c.string().map_err(CardError::Malformed)?;
    let (game, card_type) = classify_marker(&marker).ok_or(CardError::Unrecognized(marker))?;
    c.string().map_err(CardError::Malformed)?; // load version
    let face = c.i32v().map_err(CardError::Malformed)?;
    if face > 0 {
        c.take(face as usize).map_err(CardError::Malformed)?;
    }
    let n = c.i32v().map_err(CardError::Malformed)?;
    if n < 0 {
        return Err(CardError::Malformed("negative block table length".into()));
    }
    let table_bytes = c.take(n as usize).map_err(CardError::Malformed)?;
    let table = decode(table_bytes).map_err(CardError::Malformed)?;
    c.i64v().map_err(CardError::Malformed)?; // total
    let base = off + c.p as u64;

    let mut blocks = Vec::new();
    if let Some(list) = table.get("lstInfo").and_then(|v| v.as_array()) {
        for it in list {
            let name = it.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let version = it.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let pos = it.get("pos").and_then(|v| v.as_i64()).unwrap_or(-1);
            let size = it.get("size").and_then(|v| v.as_i64()).unwrap_or(-1);
            if pos < 0 || size < 0 {
                continue;
            }
            blocks.push(BlockInfo { name, version, pos: pos as u64, size: size as u64 });
        }
    }

    let mut meta = CardMeta {
        game,
        card_type,
        sex: Sex::Unknown,
        lastname: String::new(),
        firstname: String::new(),
        personality: None,
        blocks,
        base,
    };

    // Only Character cards carry a Parameter block worth reading.
    if card_type == CardType::Character {
        if let Some(bi) = meta.blocks.iter().find(|b| b.name == "Parameter") {
            let start = (c.p as u64 + bi.pos) as usize;
            let end = start.saturating_add(bi.size as usize);
            let slice = buf.get(start..end).ok_or_else(|| {
                CardError::Malformed("Parameter block runs past end of card".into())
            })?;
            let p = decode(slice).map_err(CardError::Malformed)?;
            meta.sex = match p.get("sex").and_then(|v| v.as_i64()) {
                Some(0) => Sex::Male,
                Some(1) => Sex::Female,
                _ => Sex::Unknown,
            };
            meta.lastname = p.get("lastname").and_then(|v| v.as_str()).unwrap_or("").to_string();
            meta.firstname = p.get("firstname").and_then(|v| v.as_str()).unwrap_or("").to_string();
            meta.personality = p.get("personality").and_then(|v| v.as_i64());
        }
    }
    Ok(meta)
}
```

同時把 `src-tauri/src/lib.rs` 的模組宣告補上 `pub mod card;`（放在 `pub mod core;` 之前，維持字母序）。

- [ ] **Step 4: 跑測試確認通過**

Run: `cd src-tauri && cargo test --lib card`
Expected: 6 個測試 PASS。

Run: `cd src-tauri && cargo test`
Expected: 既有測試全部仍 PASS（`round.rs`、`cli.rs` 在缺夾具時自動跳過）。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/card.rs src-tauri/src/lib.rs
git commit -m "feat(card): structured card metadata (marker, sex, name, personality)

Completes the upgrade path core.rs has recorded since the strings-based
card_strings(): a real decode of the block table and Parameter block,
starting from the offset png_char_block() already computes. Markers not
observed on a real card are reported as Unrecognized rather than guessed,
and names decode lossily because they are not guaranteed valid UTF-8."
```

---

## Task 3: 分類與排除規則（核心缺陷修正）

**Files:**
- Create: `src-tauri/src/organize.rs`
- Create: `src-tauri/tests/organize.rs`
- Modify: `src-tauri/src/lib.rs`（加 `pub mod organize;`）

**Interfaces:**
- Consumes: `crate::card::{read_card, CardMeta, CardError, CardType, DEST_FOLDERS}`
- Produces:
  - `pub struct Planned { pub from: PathBuf, pub to: PathBuf }`
  - `pub struct Unreadable { pub path: PathBuf, pub reason: String }`
  - `pub struct Plan { pub moves: Vec<Planned>, pub skipped: Vec<PathBuf>, pub unrecognized: Vec<Unreadable>, pub unreadable: Vec<Unreadable>, pub voice_incompatible: Vec<VoiceIssue> }`（`voice_incompatible` 於 Task 5 填入，本 Task 先永遠為空）
  - `pub struct VoiceIssue { pub path: PathBuf, pub personality: i64 }`
  - `pub fn destination(root: &Path, meta: &CardMeta) -> PathBuf`
  - `pub fn is_in_dest_folder(root: &Path, file: &Path) -> bool`
  - `pub fn plan(root: &Path, recursive: bool) -> Plan`

- [ ] **Step 1: 寫失敗測試**

建立 `src-tauri/tests/organize.rs`：

```rust
//! Regression tests for the card organize module. Every card is synthesised in
//! the test, so these run anywhere — no local-only fixtures involved.

use app_lib::card::fixture::card;
use app_lib::organize;
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
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `cd src-tauri && cargo test --test organize`
Expected: 編譯失敗，`unresolved import app_lib::organize`。

- [ ] **Step 3: 寫最小實作**

建立 `src-tauri/src/organize.rs`：

```rust
//! Classify Koikatsu cards into [Game]/[Male|Female] (or [Game]/[CardType])
//! folders. Everything here is read-only until `apply` (Task 4).

use crate::card::{read_card, CardError, CardMeta, CardType, DEST_FOLDERS};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct Planned {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Unreadable {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct VoiceIssue {
    pub path: PathBuf,
    pub personality: i64,
}

#[derive(Debug, Default, Serialize)]
pub struct Plan {
    pub moves: Vec<Planned>,
    pub skipped: Vec<PathBuf>,
    pub unrecognized: Vec<Unreadable>,
    pub unreadable: Vec<Unreadable>,
    pub voice_incompatible: Vec<VoiceIssue>,
}

/// Where a card belongs, relative to the scan root.
pub fn destination(root: &Path, meta: &CardMeta) -> PathBuf {
    let leaf = match meta.card_type {
        CardType::Character => meta.sex.folder(),
        other => other.folder(),
    };
    root.join(meta.game.folder()).join(leaf)
}

/// THE fix. hamster asked whether the absolute path CONTAINED a game name,
/// so `…/Koikatu_F_20260725003553199_姬野 夜王/card/` matched "Koikatu" and was
/// silently skipped — and that is exactly how Koikatsu names its own card
/// exports, i.e. how card packs are named. Ask instead whether the FIRST path
/// segment relative to the scan root IS one of the destination folders.
/// Anything upstream of the root is irrelevant by construction.
pub fn is_in_dest_folder(root: &Path, file: &Path) -> bool {
    let rel = match file.strip_prefix(root) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let first = match rel.components().next() {
        Some(c) => c.as_os_str().to_string_lossy().to_string(),
        None => return false,
    };
    DEST_FOLDERS.iter().any(|d| d.eq_ignore_ascii_case(&first))
}

fn is_png(p: &Path) -> bool {
    p.extension()
        .map(|x| x.eq_ignore_ascii_case("png"))
        .unwrap_or(false)
}

/// Depth-first walk collecting PNG files. Symlinked directories are not
/// followed (DirEntry::file_type does not follow links), so there is no cycle
/// risk. Deliberately not glob-based: card folders routinely contain `[` and
/// `]`, which a glob API would read as a character class.
fn walk(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name()); // deterministic order for tests
    for e in entries {
        let ft = match e.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            if recursive {
                walk(&e.path(), recursive, out);
            }
        } else if ft.is_file() && is_png(&e.path()) {
            out.push(e.path());
        }
    }
}

pub fn plan(root: &Path, recursive: bool) -> Plan {
    let mut files = Vec::new();
    walk(root, recursive, &mut files);
    let mut p = Plan::default();
    for f in files {
        if is_in_dest_folder(root, &f) {
            p.skipped.push(f);
            continue;
        }
        let meta = match read_card(&f) {
            Ok(m) => m,
            Err(e @ CardError::Unrecognized(_)) => {
                p.unrecognized.push(Unreadable { path: f, reason: e.reason() });
                continue;
            }
            Err(e) => {
                p.unreadable.push(Unreadable { path: f, reason: e.reason() });
                continue;
            }
        };
        let name = match f.file_name() {
            Some(n) => n.to_os_string(),
            None => continue,
        };
        let to = destination(root, &meta).join(name);
        p.moves.push(Planned { from: f, to });
    }
    p
}
```

在 `src-tauri/src/lib.rs` 加 `pub mod organize;`。

`card.rs` 的 `fixture` 模組目前是 `#[cfg(test)]`，整合測試看不到——把它改成永遠編譯但標註為測試輔助：

將 `card.rs` 中的

```rust
#[cfg(test)]
pub mod fixture {
```

改為

```rust
/// Test-only card synthesis, compiled into the lib so integration tests can use it.
pub mod fixture {
```

- [ ] **Step 4: 跑測試確認通過**

Run: `cd src-tauri && cargo test --test organize`
Expected: 8 個測試 PASS。

Run: `cd src-tauri && cargo test`
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/organize.rs src-tauri/src/card.rs src-tauri/src/lib.rs src-tauri/tests/organize.rs
git commit -m "feat(organize): classify cards, fixing the silent-skip defect

hamster excluded already-organized destination folders by substring
matching GameType names against the whole absolute path. Koikatsu's own
card export naming is Koikatu_F_<timestamp>_<name>, which is what card
packs are called, so the commonest card-pack layout was skipped silently.
The rule now compares the first path segment relative to the scan root
against the exact destination folder names.

Cards are synthesised in the tests, so the regression suite runs without
the local-only testdata fixtures."
```

---

## Task 4: 撞名處理與套用

**Files:**
- Modify: `src-tauri/src/organize.rs`
- Modify: `src-tauri/tests/organize.rs`

**Interfaces:**
- Consumes: Task 3 的 `Plan`、`Planned`
- Produces:
  - `pub enum Collision { None, AlreadyFiled, Renamed }`（加到 `Planned` 的 `pub collision: Collision` 欄位）
  - `pub struct ApplyResult { pub moved: usize, pub already_filed: usize, pub renamed: usize, pub errors: Vec<String> }`
  - `pub fn apply(plan: &Plan) -> ApplyResult`

- [ ] **Step 1: 寫失敗測試**

在 `src-tauri/tests/organize.rs` 末尾加入：

```rust
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
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `cd src-tauri && cargo test --test organize`
Expected: 編譯失敗，`no field collision on struct Planned` / `cannot find function apply`。

- [ ] **Step 3: 寫最小實作**

在 `organize.rs` 中，把 `Planned` 改為：

```rust
#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
pub enum Collision {
    /// Nothing at the destination name.
    None,
    /// Same name, byte-identical content — the card is already filed.
    AlreadyFiled,
    /// Same name, different content — file the incoming card under a new name.
    Renamed,
}

#[derive(Debug, Serialize)]
pub struct Planned {
    pub from: PathBuf,
    pub to: PathBuf,
    pub collision: Collision,
}

#[derive(Debug, Default, Serialize)]
pub struct ApplyResult {
    pub moved: usize,
    pub already_filed: usize,
    pub renamed: usize,
    pub errors: Vec<String>,
}
```

在 `organize.rs` 加入下列函式：

```rust
fn same_bytes(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(ma), Ok(mb)) if ma.len() == mb.len() => match (fs::read(a), fs::read(b)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        },
        _ => false,
    }
}

/// Windows treats `A.png` and `a.png` as one file, so a destination name must
/// be matched case-insensitively or a "new" name would silently overwrite.
fn existing_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    let rd = fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Some(e.path());
        }
    }
    None
}

/// `c.png` -> `c (2).png`, `c (3).png`, … skipping names already taken.
fn suffixed(dir: &Path, name: &str) -> PathBuf {
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    for n in 2..10_000 {
        let cand = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        if existing_case_insensitive(dir, &cand).is_none() {
            return dir.join(cand);
        }
    }
    dir.join(name)
}

pub fn apply(plan: &Plan) -> ApplyResult {
    let mut r = ApplyResult::default();
    for m in &plan.moves {
        match m.collision {
            Collision::AlreadyFiled => {
                r.already_filed += 1;
                continue;
            }
            Collision::None | Collision::Renamed => {}
        }
        let dir = match m.to.parent() {
            Some(d) => d,
            None => {
                r.errors.push(format!("{}: destination has no parent", m.to.display()));
                continue;
            }
        };
        if let Err(e) = fs::create_dir_all(dir) {
            r.errors.push(format!("{}: {e}", dir.display()));
            continue;
        }
        // rename() fails across volumes; fall back to copy + remove.
        let moved = fs::rename(&m.from, &m.to).is_ok()
            || (fs::copy(&m.from, &m.to).is_ok() && fs::remove_file(&m.from).is_ok());
        if !moved {
            r.errors.push(format!("{} -> {}: move failed", m.from.display(), m.to.display()));
            continue;
        }
        if m.collision == Collision::Renamed {
            r.renamed += 1;
        } else {
            r.moved += 1;
        }
    }
    r
}
```

最後把 `plan()` 中建立 `Planned` 的那段，由：

```rust
        let to = destination(root, &meta).join(name);
        p.moves.push(Planned { from: f, to });
```

改為：

```rust
        let dir = destination(root, &meta);
        let name_s = name.to_string_lossy().to_string();
        let (to, collision) = match existing_case_insensitive(&dir, &name_s) {
            None => (dir.join(&name_s), Collision::None),
            Some(ex) if same_bytes(&f, &ex) => (ex, Collision::AlreadyFiled),
            Some(_) => (suffixed(&dir, &name_s), Collision::Renamed),
        };
        p.moves.push(Planned { from: f, to, collision });
```

- [ ] **Step 4: 跑測試確認通過**

Run: `cd src-tauri && cargo test --test organize`
Expected: 11 個測試 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/organize.rs src-tauri/tests/organize.rs
git commit -m "feat(organize): move cards, resolving name collisions by content

hamster renamed a colliding card to c(1).png, manufacturing exactly the
duplicates this app exists to remove. Compare content instead: identical
means already filed (report, file nothing), different means a real
conflict (keep both, suffix the incoming one). Destination names are
matched case-insensitively because Windows holds A.png and a.png as one
file, so a case-sensitive check would silently overwrite."
```

---

## Task 5: 性格語音相容性

**Files:**
- Modify: `src-tauri/src/organize.rs`
- Modify: `src-tauri/tests/organize.rs`

**Interfaces:**
- Consumes: Task 3 的 `Plan`、Task 2 的 `CardMeta::personality`
- Produces:
  - `pub struct VoiceSupport { pub ids: std::collections::BTreeSet<i64>, pub source: String }`
  - `pub fn voice_support(game_root: &Path) -> VoiceSupport`
  - `plan()` 簽章改為 `pub fn plan(root: &Path, recursive: bool, voice: Option<&VoiceSupport>) -> Plan`

- [ ] **Step 1: 寫失敗測試**

在 `src-tauri/tests/organize.rs` 末尾加入：

```rust
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
```

同時把 `tests/organize.rs` 中**所有**既有的兩參數呼叫 `organize::plan(&root, <recursive>)`
一律補上第三個參數 `None`，成為 `organize::plan(&root, <recursive>, None)`。

- [ ] **Step 2: 跑測試確認失敗**

Run: `cd src-tauri && cargo test --test organize`
Expected: 編譯失敗，`cannot find function voice_support`。

- [ ] **Step 3: 寫最小實作**

在 `organize.rs` 加入：

```rust
use std::collections::BTreeSet;

/// Which personality ids the target install can actually voice.
///
/// Derived from the install, never hardcoded: a personality mod changes the
/// answer. Scope note — only the base `abdata` tree is read. Sideloader
/// zipmods could in principle add `sound/data/pcm/c<N>`, but reading them
/// needs a zip dependency this project does not carry, and a sweep of a real
/// 20,502-mod install found none doing so (the set was exactly the base
/// game's contiguous 0-38). `source` records what was actually scanned so a
/// report never overstates its own coverage.
#[derive(Debug, Clone)]
pub struct VoiceSupport {
    pub ids: BTreeSet<i64>,
    pub source: String,
}

pub fn voice_support(game_root: &Path) -> VoiceSupport {
    let dir = game_root.join("abdata").join("sound").join("data").join("pcm");
    let mut ids = BTreeSet::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // `c00`..`c38` are personalities; `c-1`, `c-100` etc. are special voices.
            if let Some(rest) = name.strip_prefix('c') {
                if let Ok(n) = rest.parse::<i64>() {
                    if n >= 0 {
                        ids.insert(n);
                    }
                }
            }
        }
    }
    VoiceSupport {
        source: format!(
            "{} ({} personality ids; base abdata only — Sideloader-added personalities are not scanned)",
            dir.display(),
            ids.len()
        ),
        ids,
    }
}
```

把 `plan()` 的簽章改為：

```rust
pub fn plan(root: &Path, recursive: bool, voice: Option<&VoiceSupport>) -> Plan {
```

並在 `plan()` 中，於推入 `p.moves` **之前**加入：

```rust
        // Only Sunshine cards are headed for conversion into KK, so only they
        // can end up voiceless there.
        if meta.game == crate::card::Game::KoikatsuSunshine {
            if let (Some(v), Some(pid)) = (voice, meta.personality) {
                if !v.ids.contains(&pid) {
                    p.voice_incompatible.push(VoiceIssue { path: f.clone(), personality: pid });
                }
            }
        }
```

（`f` 之後仍要被 move 進 `Planned`，故此處用 `f.clone()`。）

- [ ] **Step 4: 跑測試確認通過**

Run: `cd src-tauri && cargo test --test organize`
Expected: 14 個測試 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/organize.rs src-tauri/tests/organize.rs
git commit -m "feat(organize): flag KKS cards the target install cannot voice

Converting a Sunshine card whose personality id the KK install does not
have produces a card that loads correctly and has no voice at all, with
nothing said at any point. The supported set is read from the install
rather than hardcoded, because a personality mod changes it, and the
report carries the scan's own scope so it cannot overstate coverage."
```

---

## Task 6: CLI 接線

**Files:**
- Modify: `src-tauri/src/cli.rs`
- Modify: `src-tauri/tests/cli.rs`

**Interfaces:**
- Consumes: `organize::{plan, apply, voice_support}`
- Produces: `kdedupe organize --root DIR [--recursive] [--game-root DIR] [--apply]`

- [ ] **Step 1: 寫失敗測試**

在 `src-tauri/tests/cli.rs` 末尾加入：

```rust
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
}

/// Integration tests link the lib crate, so the same synthesiser the organize
/// tests use is available here — the CLI test drives the built binary but can
/// still build its input from `app_lib`.
fn app_lib_card_fixture() -> Vec<u8> {
    app_lib::card::fixture::card("【KoiKatuChara】", 1, "東山", "涼子", Some(19))
}
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `cd src-tauri && cargo test --test cli cli_organize`
Expected: 失敗——`unknown command 'organize'`，exit code 2，`json()` 的 assert 觸發。

- [ ] **Step 3: 寫最小實作**

在 `src-tauri/src/cli.rs` 頂端，把：

```rust
#[path = "core.rs"]
mod core;
```

改為：

```rust
#[path = "msgpack.rs"]
mod msgpack;
#[path = "card.rs"]
mod card;
#[path = "core.rs"]
mod core;
#[path = "organize.rs"]
mod organize;
```

`BOOL_FLAGS` 加入 `"apply"` 已存在，不需改動。

`USAGE` 的 COMMANDS 區塊，在 `delete` 那行**之後**插入：

```
  organize --root DIR [--recursive] [--game-root DIR]      DRY-RUN; add --apply to move
```

`describe()` 的 `commands` 陣列，在 `config` 條目**之前**插入：

```rust
            {"name":"organize","args":[{"name":"--root","required":true,"type":"dir"},{"name":"--recursive","type":"bool"},{"name":"--game-root","type":"dir"},{"name":"--apply","type":"bool"}],"output":"dry-run: {dry_run,moves,skipped,unrecognized,unreadable,voice_incompatible}; --apply: {moved,already_filed,renamed,errors}"},
```

`match cmd` 中，在 `"config" =>` 分支**之前**插入：

```rust
        "organize" => {
            let root = match need_root("organize") {
                Ok(r) => r,
                Err(c) => return c,
            };
            let recursive = flags.contains_key("recursive");
            let support = flags.get("game-root").map(|g| organize::voice_support(Path::new(g)));
            let p = organize::plan(&root, recursive, support.as_ref());
            if flags.contains_key("apply") {
                let r = organize::apply(&p);
                out(json!({
                    "moved": r.moved,
                    "already_filed": r.already_filed,
                    "renamed": r.renamed,
                    "errors": r.errors,
                }));
                if r.errors.is_empty() {
                    0
                } else {
                    1
                }
            } else {
                out(json!({
                    "dry_run": true,
                    "moves": p.moves,
                    "skipped": p.skipped,
                    "unrecognized": p.unrecognized,
                    "unreadable": p.unreadable,
                    "voice_incompatible": p.voice_incompatible,
                    "voice_source": support.as_ref().map(|s| s.source.clone()),
                    "hint": "re-run with --apply to move the cards",
                }));
                0
            }
        }
```

`tests/cli.rs` 需要 `app_lib`：在該檔頂端的 `use` 之後不需改動（整合測試預設可用 `app_lib`，因為 lib crate 名為 `app_lib`）。

- [ ] **Step 4: 跑測試確認通過**

Run: `cd src-tauri && cargo test --test cli`
Expected: 全部 PASS。

Run: `cd src-tauri && cargo test`
Expected: 全部 PASS，無警告。

Run: `cd src-tauri && cargo build --release`
Expected: 建置成功，產出 `target/release/kdedupe.exe`。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/cli.rs src-tauri/tests/cli.rs
git commit -m "feat(cli): add the organize command

Dry-run by default, matching delete's safety contract, and registered in
describe so an agent can discover it without being told. --game-root is
optional: without it no voice claim is made at all, rather than a guessed
supported set."
```

---

## 範圍外（另開計畫）

**GUI 面板 + i18n。** 規格要求 GUI 加一個獨立任務面板，且新字串須進 7 語 `dist/i18n.js`
（`node scripts/check-i18n.mjs` 把關）。這與本計畫的後端／CLI 是可分離的子系統：本計畫完成後
`kdedupe organize` 即為完整可用的軟體。GUI 另開計畫的另一個理由是規格已載明
`dist/index.html` 已 999 行、`i18n.js` 已 1228 行，加面板前要順手依面板拆分前端——那本身
就是一份獨立的工作。

**子專案 B（KKS→KK 轉換）與 C（流水線編排）** 見規格的〈範圍外〉一節。
