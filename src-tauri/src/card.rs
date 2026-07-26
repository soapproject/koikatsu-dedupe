//! Structured Koikatsu card metadata: marker -> (game, card type), and the
//! Parameter block's sex / name / personality. Starts from the offset
//! core::png_char_block() already computes by walking the PNG chunk chain.

use crate::msgpack::decode;
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
