//! Minimal MessagePack decoder. Decode-only, zero dependencies.

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

/// How deep a container may nest before decoding is refused. Card blocks are
/// maps of arrays of maps — three or four levels — so a real card never comes
/// close. The cap exists because the decoder recurses: a hostile or corrupt
/// block of consecutive `0x91` bytes would otherwise recurse once per byte and
/// overflow the stack, which in Rust ABORTS the process rather than unwinding.
/// A returned `Err` is the whole point: the caller already reports a malformed
/// card, and nothing in this crate may take the process down over one bad file.
const MAX_DEPTH: usize = 64;

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0, depth: 0 }
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
    /// Enters one container level, refusing to recurse past `MAX_DEPTH`. On
    /// the error path the depth counter is deliberately not restored: the
    /// whole decode is being abandoned, and every frame returns `Err`.
    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!("msgpack nested deeper than {MAX_DEPTH} levels"));
        }
        Ok(())
    }
    fn array(&mut self, n: usize) -> Result<Value, String> {
        self.enter()?;
        let mut v = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            v.push(self.read()?);
        }
        self.depth -= 1;
        Ok(Value::Array(v))
    }
    fn map(&mut self, n: usize) -> Result<Value, String> {
        self.enter()?;
        let mut v = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            let k = self.read()?;
            let val = self.read()?;
            v.push((k, val));
        }
        self.depth -= 1;
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

    /// The decoder recurses per container, so deeply nested input would blow
    /// the stack — and a stack overflow in Rust ABORTS the process, which no
    /// malformed card may be allowed to do. It must be an ordinary `Err`.
    #[test]
    fn pathological_nesting_is_an_error_not_a_stack_overflow() {
        let mut buf = vec![0x91u8; 100_000]; // 100k nested fixarray(1)
        buf.push(0x00); // ...eventually holding a single int
        let e = decode(&buf).expect_err("must refuse, not recurse");
        assert!(e.contains("nested deeper"), "{e}");

        // The same shape built from maps.
        let mut m = vec![0x81u8; 100_000];
        m.extend_from_slice(&[0xA1, b'k', 0x00]);
        assert!(decode(&m).is_err());
    }

    /// The cap must not reject the nesting real cards use. A card's block
    /// table is a map of an array of maps — three levels — so anything within
    /// an order of magnitude of that has to keep decoding.
    #[test]
    fn nesting_within_the_cap_still_decodes() {
        let mut buf = vec![0x91u8; 60];
        buf.push(0x07);
        let v = decode(&buf).expect("60 levels is well inside the cap");
        // Unwrap all 60 arrays and find the int.
        let mut cur = &v;
        for _ in 0..60 {
            cur = &cur.as_array().expect("array")[0];
        }
        assert_eq!(cur.as_i64(), Some(7));
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
