//! Steam's two VDF formats.
//!
//! * Binary VDF: `userdata/<id>/config/shortcuts.vdf` (the non-Steam games in your library).
//! * Text VDF: `config/config.vdf` (Proton choice per game), `libraryfolders.vdf`, `appmanifest_*.acf`.
//!
//! Both are kept in insertion order so a load/save round trip leaves Steam's files as Steam wrote them.

use anyhow::{bail, Context, Result};

// ---------------------------------------------------------------- binary

#[derive(Debug, Clone, PartialEq)]
pub enum BValue {
    Map(BMap),
    Str(String),
    I32(i32),
    F32(f32),
    U64(u64),
}

/// An ordered map, as Steam writes it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BMap(pub Vec<(String, BValue)>);

impl BMap {
    pub fn get(&self, key: &str) -> Option<&BValue> {
        self.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut BValue> {
        self.0.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }
    pub fn set(&mut self, key: &str, v: BValue) {
        match self.get_mut(key) {
            Some(slot) => *slot = v,
            None => self.0.push((key.to_string(), v)),
        }
    }
    pub fn str(&self, key: &str) -> &str {
        match self.get(key) {
            Some(BValue::Str(s)) => s,
            _ => "",
        }
    }
    pub fn i32(&self, key: &str) -> Option<i32> {
        match self.get(key) {
            Some(BValue::I32(v)) => Some(*v),
            _ => None,
        }
    }
    pub fn map(&self, key: &str) -> Option<&BMap> {
        match self.get(key) {
            Some(BValue::Map(m)) => Some(m),
            _ => None,
        }
    }
    pub fn map_mut(&mut self, key: &str) -> &mut BMap {
        if !matches!(self.get(key), Some(BValue::Map(_))) {
            self.set(key, BValue::Map(BMap::default()));
        }
        match self.get_mut(key) {
            Some(BValue::Map(m)) => m,
            _ => unreachable!(),
        }
    }
}

pub fn binary_load(data: &[u8]) -> Result<BMap> {
    let mut pos = 0usize;
    fn cstr(data: &[u8], pos: &mut usize) -> Result<String> {
        let end = data[*pos..].iter().position(|&b| b == 0).context("unterminated string")? + *pos;
        let s = String::from_utf8_lossy(&data[*pos..end]).into_owned();
        *pos = end + 1;
        Ok(s)
    }
    fn take<const N: usize>(data: &[u8], pos: &mut usize) -> Result<[u8; N]> {
        let b: [u8; N] = data.get(*pos..*pos + N).context("file ends early")?.try_into()?;
        *pos += N;
        Ok(b)
    }
    fn map(data: &[u8], pos: &mut usize) -> Result<BMap> {
        let mut out = BMap::default();
        loop {
            let t = *data.get(*pos).context("file ends early")?;
            *pos += 1;
            if t == 0x08 {
                return Ok(out);
            }
            let key = cstr(data, pos)?;
            let v = match t {
                0x00 => BValue::Map(map(data, pos)?),
                0x01 => BValue::Str(cstr(data, pos)?),
                0x02 => BValue::I32(i32::from_le_bytes(take::<4>(data, pos)?)),
                0x03 => BValue::F32(f32::from_le_bytes(take::<4>(data, pos)?)),
                0x07 => BValue::U64(u64::from_le_bytes(take::<8>(data, pos)?)),
                _ => bail!("unknown VDF type {t:#x} at byte {pos}"),
            };
            out.0.push((key, v));
        }
    }
    map(data, &mut pos)
}

pub fn binary_dump(m: &BMap) -> Vec<u8> {
    fn w(m: &BMap, out: &mut Vec<u8>) {
        for (k, v) in &m.0 {
            let key = |out: &mut Vec<u8>, t: u8| {
                out.push(t);
                out.extend_from_slice(k.as_bytes());
                out.push(0);
            };
            match v {
                BValue::Map(sub) => {
                    key(out, 0x00);
                    w(sub, out);
                    out.push(0x08);
                }
                BValue::Str(s) => {
                    key(out, 0x01);
                    out.extend_from_slice(s.as_bytes());
                    out.push(0);
                }
                BValue::I32(i) => {
                    key(out, 0x02);
                    out.extend_from_slice(&i.to_le_bytes());
                }
                BValue::F32(f) => {
                    key(out, 0x03);
                    out.extend_from_slice(&f.to_le_bytes());
                }
                BValue::U64(u) => {
                    key(out, 0x07);
                    out.extend_from_slice(&u.to_le_bytes());
                }
            }
        }
    }
    let mut out = Vec::new();
    w(m, &mut out);
    out.push(0x08);
    out
}

// ---------------------------------------------------------------- text

#[derive(Debug, Clone, PartialEq)]
pub enum TValue {
    Str(String),
    Block(TBlock),
}

/// Ordered list of key/value pairs; duplicate keys survive.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TBlock(pub Vec<(String, TValue)>);

impl TBlock {
    pub fn block(&self, key: &str) -> Option<&TBlock> {
        self.0.iter().find_map(|(k, v)| match v {
            TValue::Block(b) if k.eq_ignore_ascii_case(key) => Some(b),
            _ => None,
        })
    }
    pub fn block_mut(&mut self, key: &str) -> &mut TBlock {
        let idx = self
            .0
            .iter()
            .position(|(k, v)| k.eq_ignore_ascii_case(key) && matches!(v, TValue::Block(_)));
        let idx = match idx {
            Some(i) => i,
            None => {
                self.0.push((key.to_string(), TValue::Block(TBlock::default())));
                self.0.len() - 1
            }
        };
        match &mut self.0[idx].1 {
            TValue::Block(b) => b,
            _ => unreachable!(),
        }
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        self.0.iter().find_map(|(k, v)| match v {
            TValue::Str(s) if k.eq_ignore_ascii_case(key) => Some(s.as_str()),
            _ => None,
        })
    }
    pub fn remove(&mut self, key: &str) {
        self.0.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    }
    /// Follow a path of block names, creating missing ones.
    pub fn path_mut(&mut self, path: &[&str]) -> &mut TBlock {
        let mut cur = self;
        for p in path {
            cur = cur.block_mut(p);
        }
        cur
    }
    pub fn path(&self, path: &[&str]) -> Option<&TBlock> {
        let mut cur = self;
        for p in path {
            cur = cur.block(p)?;
        }
        Some(cur)
    }
}

pub fn text_load(text: &str) -> Result<TBlock> {
    #[derive(Debug, PartialEq)]
    enum Tok {
        Open,
        Close,
        S(String),
    }
    let mut toks = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '{' {
            toks.push(Tok::Open);
            i += 1;
        } else if c == '}' {
            toks.push(Tok::Close);
            i += 1;
        } else if c == '"' {
            let mut j = i + 1;
            let mut buf = String::new();
            while j < chars.len() && chars[j] != '"' {
                if chars[j] == '\\' && j + 1 < chars.len() {
                    buf.push(match chars[j + 1] {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                    j += 2;
                } else {
                    buf.push(chars[j]);
                    j += 1;
                }
            }
            toks.push(Tok::S(buf));
            i = j + 1;
        } else {
            let mut j = i;
            while j < chars.len() && !chars[j].is_whitespace() && !"{}\"".contains(chars[j]) {
                j += 1;
            }
            toks.push(Tok::S(chars[i..j].iter().collect()));
            i = j;
        }
    }
    let mut pos = 0;
    fn block(toks: &[Tok], pos: &mut usize) -> Result<TBlock> {
        let mut out = TBlock::default();
        while *pos < toks.len() {
            let key = match &toks[*pos] {
                Tok::Close => {
                    *pos += 1;
                    return Ok(out);
                }
                Tok::S(s) => s.clone(),
                Tok::Open => bail!("unexpected '{{'"),
            };
            *pos += 1;
            match toks.get(*pos) {
                Some(Tok::Open) => {
                    *pos += 1;
                    out.0.push((key, TValue::Block(block(toks, pos)?)));
                }
                Some(Tok::S(v)) => {
                    out.0.push((key, TValue::Str(v.clone())));
                    *pos += 1;
                    // conditionals like [$WIN32] are dropped
                    if let Some(Tok::S(n)) = toks.get(*pos) {
                        if n.starts_with('[') {
                            *pos += 1;
                        }
                    }
                }
                _ => bail!("key {key:?} has no value"),
            }
        }
        Ok(out)
    }
    block(&toks, &mut pos)
}

pub fn text_dump(b: &TBlock) -> String {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    fn w(b: &TBlock, depth: usize, out: &mut String) {
        let tab = "\t".repeat(depth);
        for (k, v) in &b.0 {
            match v {
                TValue::Block(sub) => {
                    out.push_str(&format!("{tab}\"{}\"\n{tab}{{\n", esc(k)));
                    w(sub, depth + 1, out);
                    out.push_str(&format!("{tab}}}\n"));
                }
                TValue::Str(s) => out.push_str(&format!("{tab}\"{}\"\t\t\"{}\"\n", esc(k), esc(s))),
            }
        }
    }
    let mut out = String::new();
    w(b, 0, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_round_trip() {
        let mut inner = BMap::default();
        inner.set("appid", BValue::I32(-123));
        inner.set("AppName", BValue::Str("Super Mario 63".into()));
        inner.map_mut("tags").set("0", BValue::Str("Super Mario Bros".into()));
        let mut sc = BMap::default();
        sc.map_mut("shortcuts").set("0", BValue::Map(inner));
        let bytes = binary_dump(&sc);
        assert_eq!(binary_load(&bytes).unwrap(), sc);
    }

    #[test]
    fn text_round_trip() {
        let src = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"a\"\t\t\"b \\\"q\\\"\"\n\t}\n}\n";
        let t = text_load(src).unwrap();
        assert_eq!(t.path(&["InstallConfigStore", "Software"]).unwrap().str("a"), Some("b \"q\""));
        assert_eq!(text_load(&text_dump(&t)).unwrap(), t);
    }
}
