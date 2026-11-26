//! The game's files in a local CASC storage, as Battle.net installs
//! Warcraft III Reforged: read, never written.
//!
//! - `.build.info` (in the game's folder): a table, `|` between cells,
//!   each header `Name!TYPE:size`; the active row's `Build Key`;
//! - the build's configuration (`Data/config/ab/cd/<key>`): `key = value`
//!   lines; `encoding`'s second key (its encoded key) and `root`'s (a
//!   content key);
//! - the local indices (`Data/data/BBVVVVVVVV.idx`: bucket `BB`, the
//!   highest version `VVVVVVVV` of each): each entry an encoded key's
//!   first 9 bytes, where its data is (5 bytes big-endian: the archive
//!   `data.NNN` above `bits`, the offset below) and its size;
//! - an entry's data: a 30-byte header, then BLTE ([`blte`]);
//! - the encoding file: each content key's encoded keys;
//! - the root (Warcraft III's: text, a line a file, `name|content key|..`;
//!   names `War3.w3mod:Units/Human/Footman/Footman.mdx`, a mod's files
//!   under it, `War3.w3mod:_HD.w3mod:...`).
//!
//! Only what the storage has on disk can be read: a locale not
//! installed is in the root, its data isn't ([`Storage::has`]).

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// What went wrong, said.
#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn err<T>(message: impl Into<String>) -> Result<T> {
    Err(Error(message.into()))
}

/// Where an encoded key's data is.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Location {
    archive: u32,
    offset: u64,
    size: u32,
}

/// A file of the root: its name as the root has it, its content key.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub ckey: [u8; 16],
}

/// A local CASC storage, open.
pub struct Storage {
    data: PathBuf,
    index: HashMap<[u8; 9], Location>,
    encoding: HashMap<[u8; 16], [u8; 16]>,
    /// The root's files, in its order.
    pub entries: Vec<Entry>,
    /// Each name (lower case, `/` between folders) to its entry.
    names: HashMap<String, usize>,
    /// The build's name (`build-name`: `3.0.1.24371-retail`).
    pub build: String,
}

/// `name` as names are looked up: lower case, `/` between folders.
pub fn key(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

fn hex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return err(format!("{s} isn't hex"));
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| Error(format!("{s} isn't hex")))).collect()
}

fn key16(s: &str) -> Result<[u8; 16]> {
    hex(s)?.try_into().map_err(|_| Error(format!("{s} isn't a key of 16 bytes")))
}

/// The active build's key in `.build.info`.
fn build_key(folder: &Path) -> Result<String> {
    let path = folder.join(".build.info");
    let text = std::fs::read_to_string(&path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<&str> = lines.next().ok_or_else(|| Error(format!("{} is empty", path.display())))?.split('|').map(|h| h.split('!').next().unwrap_or(h)).collect();
    let col = |name: &str| header.iter().position(|h| *h == name);
    let (Some(key), active) = (col("Build Key"), col("Active")) else { return err(format!("{} has no Build Key", path.display())) };
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split('|').collect()).collect();
    let row = rows.iter().find(|r| active.is_some_and(|a| r.get(a) == Some(&"1"))).or(rows.first()).ok_or_else(|| Error(format!("{} has no build", path.display())))?;
    Ok(row.get(key).ok_or_else(|| Error(format!("{}: no Build Key in its row", path.display())))?.to_string())
}

/// A configuration file of `Data/config`: its `key = value` lines.
fn config(data: &Path, key: &str) -> Result<HashMap<String, Vec<String>>> {
    let path = data.join("config").join(&key[0..2]).join(&key[2..4]).join(key);
    let text = std::fs::read_to_string(&path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    Ok(text
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.split_whitespace().map(str::to_string).collect()))
        })
        .collect())
}

/// The local indices: the highest version of each bucket's `.idx`.
fn indices(data: &Path) -> Result<HashMap<[u8; 9], Location>> {
    let dir = data.join("data");
    let mut best: HashMap<u8, (u32, PathBuf)> = HashMap::new();
    for e in std::fs::read_dir(&dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(".idx").filter(|s| s.len() == 10) else { continue };
        let (Ok(bucket), Ok(version)) = (u8::from_str_radix(&stem[..2], 16), u32::from_str_radix(&stem[2..], 16)) else { continue };
        if best.get(&bucket).is_none_or(|(v, _)| version > *v) {
            best.insert(bucket, (version, e.path()));
        }
    }
    let mut index = HashMap::new();
    for (_, path) in best.values() {
        let d = std::fs::read(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        read_index(&d, &mut index).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    }
    Ok(index)
}

/// One `.idx` file's entries into `index` (the first one met kept).
fn read_index(d: &[u8], index: &mut HashMap<[u8; 9], Location>) -> Result<()> {
    let u32le = |at: usize| -> Result<u32> { Ok(u32::from_le_bytes(d.get(at..at + 4).ok_or_else(|| Error("cut short".into()))?.try_into().unwrap())) };
    let header_size = u32le(0)? as usize;
    let field = |at: usize| d.get(at).copied().ok_or_else(|| Error("cut short".into()));
    let (size_len, loc_len, key_len, bits) = (field(12)? as usize, field(13)? as usize, field(14)? as usize, field(15)? as u32);
    if key_len != 9 || loc_len != 5 || size_len != 4 {
        return err(format!("entries of {key_len}, {loc_len}, {size_len} bytes: only 9, 5, 4 are known"));
    }
    let mut at = (8 + header_size).div_ceil(16) * 16;
    let entries = u32le(at)? as usize;
    at += 8;
    let n = entries / 18;
    for i in 0..n {
        let e = d.get(at + i * 18..at + i * 18 + 18).ok_or_else(|| Error("cut short".into()))?;
        let key: [u8; 9] = e[..9].try_into().unwrap();
        let loc = e[9..14].iter().fold(0u64, |v, b| v << 8 | *b as u64);
        let size = u32::from_le_bytes(e[14..18].try_into().unwrap());
        index.entry(key).or_insert(Location { archive: (loc >> bits) as u32, offset: loc & ((1u64 << bits) - 1), size });
    }
    Ok(())
}

/// BLTE's data decoded: its chunks, each raw (`N`) or zlib (`Z`);
/// another kind (encrypted `E`, a frame `F`, LZ4 `4`) said.
pub fn blte(b: &[u8]) -> Result<Vec<u8>> {
    if b.get(..4) != Some(b"BLTE") {
        return err("not BLTE");
    }
    let be = |at: usize| -> Result<u32> { Ok(u32::from_be_bytes(b.get(at..at + 4).ok_or_else(|| Error("BLTE cut short".into()))?.try_into().unwrap())) };
    let header = be(4)? as usize;
    let mut chunks: Vec<usize> = Vec::new();
    let mut at;
    if header == 0 {
        chunks.push(b.len() - 8);
        at = 8;
    } else {
        let count = (be(8)? & 0xFF_FFFF) as usize;
        for i in 0..count {
            chunks.push(be(12 + i * 24)? as usize);
        }
        at = header;
    }
    let mut out = Vec::new();
    for size in chunks {
        let c = b.get(at..at + size).ok_or_else(|| Error("BLTE chunk cut short".into()))?;
        at += size;
        match c.first() {
            Some(b'N') => out.extend_from_slice(&c[1..]),
            Some(b'Z') => {
                flate2::read::ZlibDecoder::new(&c[1..]).read_to_end(&mut out).map_err(|e| Error(format!("BLTE zlib: {e}")))?;
            }
            Some(m) => return err(format!("a BLTE chunk of kind {:?}, not read", *m as char)),
            None => {}
        }
    }
    Ok(out)
}

/// The encoding file's content keys, each to its first encoded key.
fn encoding(e: &[u8]) -> Result<HashMap<[u8; 16], [u8; 16]>> {
    if e.get(..2) != Some(b"EN") {
        return err("the encoding file isn't one");
    }
    let be16 = |at: usize| u16::from_be_bytes([e[at], e[at + 1]]) as usize;
    let be32 = |at: usize| u32::from_be_bytes(e[at..at + 4].try_into().unwrap()) as usize;
    if e.len() < 22 {
        return err("the encoding file is cut short");
    }
    let (ckey_size, ekey_size) = (e[3] as usize, e[4] as usize);
    if ckey_size != 16 || ekey_size != 16 {
        return err("keys other than 16 bytes in the encoding file");
    }
    let page = be16(5) * 1024;
    let pages = be32(9);
    let espec = be32(18);
    let first = 22 + espec + pages * 32;
    let mut out = HashMap::new();
    for p in 0..pages {
        let mut at = first + p * page;
        let end = (at + page).min(e.len());
        while at + 22 <= end {
            let count = e[at] as usize;
            if count == 0 || at + 22 + count * 16 > end {
                break;
            }
            let ckey: [u8; 16] = e[at + 6..at + 22].try_into().unwrap();
            let ekey: [u8; 16] = e[at + 22..at + 38].try_into().unwrap();
            out.insert(ckey, ekey);
            at += 22 + count * 16;
        }
    }
    Ok(out)
}

impl Storage {
    /// The storage of the game's folder (the one with `.build.info`).
    pub fn open(folder: &Path) -> Result<Storage> {
        let data = folder.join("Data");
        let build = config(&data, &build_key(folder)?)?;
        let get = |k: &str, i: usize| build.get(k).and_then(|v| v.get(i)).cloned().ok_or_else(|| Error(format!("the build's configuration has no {k}")));
        let index = indices(&data)?;
        let mut s = Storage { data, index, encoding: HashMap::new(), entries: Vec::new(), names: HashMap::new(), build: get("build-name", 0).unwrap_or_default() };
        let encoding_ekey = key16(&get("encoding", 1)?)?;
        s.encoding = encoding(&s.read_ekey(&encoding_ekey)?)?;
        let root = s.read_ckey(&key16(&get("root", 0)?)?)?;
        for line in String::from_utf8_lossy(&root).lines() {
            let mut cells = line.split('|');
            let (Some(name), Some(ckey)) = (cells.next(), cells.next()) else { continue };
            let Ok(ckey) = key16(ckey.trim()) else { continue };
            s.names.insert(key(name), s.entries.len());
            s.entries.push(Entry { name: name.to_string(), ckey });
        }
        Ok(s)
    }

    /// The data of an encoded key, decoded.
    fn read_ekey(&self, ekey: &[u8; 16]) -> Result<Vec<u8>> {
        let k: [u8; 9] = ekey[..9].try_into().unwrap();
        let loc = self.index.get(&k).ok_or_else(|| Error("not in the storage on disk".into()))?;
        let path = self.data.join("data").join(format!("data.{:03}", loc.archive));
        let mut f = std::fs::File::open(&path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        f.seek(SeekFrom::Start(loc.offset + 30)).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let mut b = vec![0u8; (loc.size as usize).saturating_sub(30)];
        f.read_exact(&mut b).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        blte(&b)
    }

    fn read_ckey(&self, ckey: &[u8; 16]) -> Result<Vec<u8>> {
        let ekey = self.encoding.get(ckey).ok_or_else(|| Error("not in the encoding".into()))?;
        self.read_ekey(ekey)
    }

    fn entry(&self, name: &str) -> Option<&Entry> {
        self.names.get(&key(name)).map(|i| &self.entries[*i])
    }

    /// Does the root name `name` (any case, `\` or `/`), and is its data
    /// on disk?
    pub fn has(&self, name: &str) -> bool {
        self.entry(name).is_some_and(|e| self.encoding.get(&e.ckey).is_some_and(|ek| self.index.contains_key(&<[u8; 9]>::try_from(&ek[..9]).unwrap())))
    }

    /// File `name`'s bytes (`War3.w3mod:Units/Human/Footman/Footman.mdx`).
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let e = self.entry(name).ok_or_else(|| Error(format!("{name}: not in the root")))?;
        self.read_ckey(&e.ckey).map_err(|err| Error(format!("{name}: {err}")))
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
