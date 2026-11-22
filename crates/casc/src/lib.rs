//! Read-only access to a local CASC storage, as installed by the game.
//!
//! The chain from a file to its bytes:
//!
//! 1. `.build.info` names the active build config for a product.
//! 2. The build config names the encoding and root files by content key.
//! 3. The root file maps FileDataIDs (and hashes of file paths) to content keys.
//! 4. The encoding file maps content keys to encoding keys.
//! 5. The `.idx` index files map encoding keys to a place in a `data.NNN` archive.
//! 6. The data there is BLTE encoded (raw or zlib chunks).
//!
//! Nothing is ever written to the storage.

mod blte;
mod jenkins;

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub use blte::decode as decode_blte;
pub use jenkins::path_hash;

pub type Key = [u8; 16];

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Format(String),
    NotFound(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Format(s) => write!(f, "bad CASC data: {s}"),
            Error::NotFound(s) => write!(f, "not found: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn format_err(s: impl Into<String>) -> Error {
    Error::Format(s.into())
}

/// Location of an encoded file inside the data archives.
#[derive(Clone, Copy, Debug)]
struct Location {
    archive: u16,
    offset: u32,
    size: u32,
}

pub struct Storage {
    data_dir: PathBuf,
    /// Sorted by the first 9 bytes of the encoding key.
    index: Vec<([u8; 9], Location)>,
    encoding: Encoding,
    /// Sorted by FileDataID.
    root: Vec<(u32, Key)>,
    /// Sorted by path hash.
    names: Vec<(u64, u32)>,
    archives: Mutex<HashMap<u16, File>>,
    pub build_name: String,
}

impl Storage {
    /// Open the storage of a game install. `install_dir` is the folder that
    /// contains `.build.info` and `Data`; `product` is for example `wow`.
    pub fn open(install_dir: &Path, product: &str) -> Result<Storage> {
        let build_info = std::fs::read_to_string(install_dir.join(".build.info"))?;
        let build_key = build_key(&build_info, product)
            .ok_or_else(|| Error::NotFound(format!("product {product} in .build.info")))?;
        let data_dir = install_dir.join("Data");
        let config = read_config(&data_dir, &build_key)?;
        let encoding_ekey = config
            .get("encoding")
            .and_then(|v| v.split_whitespace().nth(1))
            .and_then(parse_hex_key)
            .ok_or_else(|| format_err("build config has no encoding key"))?;
        let root_ckey = config
            .get("root")
            .and_then(|v| parse_hex_key(v.trim()))
            .ok_or_else(|| format_err("build config has no root key"))?;

        let mut storage = Storage {
            index: read_indices(&data_dir.join("data"))?,
            data_dir,
            encoding: Encoding::default(),
            root: Vec::new(),
            names: Vec::new(),
            archives: Mutex::new(HashMap::new()),
            build_name: config.get("build-name").cloned().unwrap_or_default(),
        };
        let encoding = storage.read_by_ekey(&encoding_ekey)?;
        storage.encoding = Encoding::parse(encoding)?;
        let root_ekey = storage
            .encoding
            .ekey(&root_ckey)
            .ok_or_else(|| format_err("root file missing from encoding"))?;
        let root = storage.read_by_ekey(&root_ekey)?;
        let (root, names) = parse_root(&root)?;
        storage.root = root;
        storage.names = names;
        Ok(storage)
    }

    /// Number of files in the root table (for the selected locale).
    pub fn file_count(&self) -> usize {
        self.root.len()
    }

    /// FileDataID of a path such as `Interface\Icons\Foo.blp`.
    pub fn file_id(&self, path: &str) -> Option<u32> {
        let hash = path_hash(path);
        self.names
            .binary_search_by_key(&hash, |(h, _)| *h)
            .ok()
            .map(|i| self.names[i].1)
    }

    pub fn has_file(&self, file_id: u32) -> bool {
        self.root
            .binary_search_by_key(&file_id, |(id, _)| *id)
            .is_ok()
    }

    /// Read a file by FileDataID.
    pub fn read_file(&self, file_id: u32) -> Result<Vec<u8>> {
        let ckey = self
            .root
            .binary_search_by_key(&file_id, |(id, _)| *id)
            .map(|i| self.root[i].1)
            .map_err(|_| Error::NotFound(format!("FileDataID {file_id}")))?;
        let ekey = self
            .encoding
            .ekey(&ckey)
            .ok_or_else(|| Error::NotFound(format!("encoding for FileDataID {file_id}")))?;
        self.read_by_ekey(&ekey)
    }

    /// Read a file by path.
    pub fn read_path(&self, path: &str) -> Result<Vec<u8>> {
        let id = self
            .file_id(path)
            .ok_or_else(|| Error::NotFound(path.to_string()))?;
        self.read_file(id)
    }

    fn read_by_ekey(&self, ekey: &Key) -> Result<Vec<u8>> {
        let mut short = [0u8; 9];
        short.copy_from_slice(&ekey[..9]);
        let loc = self
            .index
            .binary_search_by_key(&short, |(k, _)| *k)
            .map(|i| self.index[i].1)
            .map_err(|_| Error::NotFound("encoding key not in local index".into()))?;
        let mut archives = self.archives.lock().unwrap_or_else(|e| e.into_inner());
        let file = match archives.entry(loc.archive) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let path = self
                    .data_dir
                    .join("data")
                    .join(format!("data.{:03}", loc.archive));
                e.insert(File::open(path)?)
            }
        };
        // Each entry starts with a 30 byte header: reversed key, size, flags
        // and checksums. The BLTE data follows.
        const HEADER: u32 = 30;
        if loc.size < HEADER {
            return Err(format_err("archive entry too small"));
        }
        file.seek(SeekFrom::Start(loc.offset as u64 + HEADER as u64))?;
        let mut buf = vec![0u8; (loc.size - HEADER) as usize];
        file.read_exact(&mut buf)?;
        drop(archives);
        blte::decode(&buf)
    }
}

fn build_key(build_info: &str, product: &str) -> Option<String> {
    let mut lines = build_info.lines();
    let header: Vec<String> = lines
        .next()?
        .split('|')
        .map(|h| h.split('!').next().unwrap_or("").to_string())
        .collect();
    let col = |name: &str| header.iter().position(|h| h == name);
    let (key_col, product_col, active_col) = (col("Build Key")?, col("Product")?, col("Active"));
    lines
        .map(|l| l.split('|').collect::<Vec<_>>())
        .filter(|row| row.get(product_col) == Some(&product))
        .find(|row| active_col.is_none_or(|c| row.get(c) == Some(&"1")))
        .and_then(|row| row.get(key_col).map(|s| s.to_string()))
}

fn read_config(data_dir: &Path, key: &str) -> Result<HashMap<String, String>> {
    if key.len() < 4 {
        return Err(format_err("short config key"));
    }
    let path = data_dir
        .join("config")
        .join(&key[0..2])
        .join(&key[2..4])
        .join(key);
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect())
}

fn parse_hex_key(s: &str) -> Option<Key> {
    if s.len() != 32 {
        return None;
    }
    let mut key = [0u8; 16];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(key)
}

/// Read the newest `.idx` file of each of the 16 buckets.
fn read_indices(dir: &Path) -> Result<Vec<([u8; 9], Location)>> {
    let mut newest: HashMap<u8, (u32, PathBuf)> = HashMap::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let Some(stem) = name.strip_suffix(".idx") else {
            continue;
        };
        if stem.len() != 10 {
            continue;
        }
        let (Ok(bucket), Ok(version)) = (
            u8::from_str_radix(&stem[0..2], 16),
            u32::from_str_radix(&stem[2..], 16),
        ) else {
            continue;
        };
        let e = newest.entry(bucket).or_insert((version, path.clone()));
        if version >= e.0 {
            *e = (version, path);
        }
    }
    let mut out = Vec::new();
    for (_, path) in newest.values() {
        let data = std::fs::read(path)?;
        parse_index(&data, &mut out)?;
    }
    out.sort_by_key(|(k, _)| *k);
    out.dedup_by_key(|(k, _)| *k);
    Ok(out)
}

fn parse_index(data: &[u8], out: &mut Vec<([u8; 9], Location)>) -> Result<()> {
    if data.len() < 0x28 {
        return Err(format_err("index file too small"));
    }
    let size_len = data[12] as usize;
    let offset_len = data[13] as usize;
    let key_len = data[14] as usize;
    let offset_bits = data[15] as u32;
    if key_len != 9 || offset_len != 5 || size_len != 4 {
        return Err(format_err("unsupported index layout"));
    }
    let entries_size = u32::from_le_bytes(data[0x20..0x24].try_into().unwrap()) as usize;
    let entry_len = key_len + offset_len + size_len;
    let start = 0x28;
    let end = (start + entries_size).min(data.len());
    for e in data[start..end].chunks_exact(entry_len) {
        let mut key = [0u8; 9];
        key.copy_from_slice(&e[..9]);
        let mut packed = 0u64;
        for b in &e[9..14] {
            packed = (packed << 8) | *b as u64;
        }
        let size = u32::from_le_bytes(e[14..18].try_into().unwrap());
        out.push((
            key,
            Location {
                archive: (packed >> offset_bits) as u16,
                offset: (packed & ((1u64 << offset_bits) - 1)) as u32,
                size,
            },
        ));
    }
    Ok(())
}

/// The encoding table, kept as raw pages for binary search.
#[derive(Default)]
struct Encoding {
    data: Vec<u8>,
    page_size: usize,
    /// First content key of each page.
    page_keys: Vec<Key>,
    pages_start: usize,
}

impl Encoding {
    fn parse(data: Vec<u8>) -> Result<Encoding> {
        if data.len() < 22 || &data[0..2] != b"EN" {
            return Err(format_err("bad encoding header"));
        }
        let ckey_size = data[3] as usize;
        let ekey_size = data[4] as usize;
        if ckey_size != 16 || ekey_size != 16 {
            return Err(format_err("unsupported encoding key size"));
        }
        let page_kb = u16::from_be_bytes([data[5], data[6]]) as usize;
        let page_count = u32::from_be_bytes(data[9..13].try_into().unwrap()) as usize;
        let espec_size = u32::from_be_bytes(data[18..22].try_into().unwrap()) as usize;
        let index_start = 22 + espec_size;
        let pages_start = index_start + page_count * 32;
        let page_size = page_kb * 1024;
        if data.len() < pages_start + page_count * page_size {
            return Err(format_err("encoding file truncated"));
        }
        let page_keys = (0..page_count)
            .map(|i| {
                let o = index_start + i * 32;
                data[o..o + 16].try_into().unwrap()
            })
            .collect();
        Ok(Encoding {
            data,
            page_size,
            page_keys,
            pages_start,
        })
    }

    fn ekey(&self, ckey: &Key) -> Option<Key> {
        let page = match self.page_keys.binary_search(ckey) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let start = self.pages_start + page * self.page_size;
        let page_data = &self.data[start..start + self.page_size];
        let mut o = 0;
        while o + 22 <= page_data.len() {
            let count = page_data[o] as usize;
            if count == 0 {
                break;
            }
            let key = &page_data[o + 6..o + 22];
            if key == ckey && o + 38 <= page_data.len() {
                return page_data[o + 22..o + 38].try_into().ok();
            }
            o += 22 + count * 16;
        }
        None
    }
}

const LOCALE_ENUS: u32 = 0x2;
const LOCALE_ALL: u32 = 0xffff_ffff;
const CONTENT_NO_NAME_HASH: u32 = 0x1000_0000;
const CONTENT_LOW_VIOLENCE: u32 = 0x80;

/// Parse the root file (the `TSFM` format used since 8.2).
#[allow(clippy::type_complexity)]
fn parse_root(data: &[u8]) -> Result<(Vec<(u32, Key)>, Vec<(u64, u32)>)> {
    let u32_at = |o: usize| -> Result<u32> {
        data.get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| format_err("root file truncated"))
    };
    if data.len() < 12 || &data[0..4] != b"TSFM" {
        return Err(format_err("unsupported root format"));
    }
    // Newer files have a header size and version after the magic; older ones
    // have the file counts directly.
    let (mut o, version) = {
        let second = u32_at(4)?;
        if second < 100 {
            (second as usize, u32_at(8)?)
        } else {
            (12, 0)
        }
    };
    let mut root: HashMap<u32, Key> = HashMap::new();
    let mut names: HashMap<u64, u32> = HashMap::new();
    while o + 12 <= data.len() {
        let count = u32_at(o)? as usize;
        let (content_flags, locale_flags);
        if version >= 2 {
            locale_flags = u32_at(o + 4)?;
            let f1 = u32_at(o + 8)?;
            let f2 = u32_at(o + 12)?;
            let f3 = *data
                .get(o + 16)
                .ok_or_else(|| format_err("root truncated"))? as u32;
            content_flags = f1 | f2 | (f3 << 17);
            o += 17;
        } else {
            content_flags = u32_at(o + 4)?;
            locale_flags = u32_at(o + 8)?;
            o += 12;
        }
        let has_names = content_flags & CONTENT_NO_NAME_HASH == 0;
        let ids_end = o + count * 4;
        let keys_end = ids_end + count * 16;
        let names_end = if has_names {
            keys_end + count * 8
        } else {
            keys_end
        };
        if names_end > data.len() {
            return Err(format_err("root block truncated"));
        }
        let wanted = (locale_flags & LOCALE_ENUS != 0 || locale_flags == LOCALE_ALL)
            && content_flags & CONTENT_LOW_VIOLENCE == 0;
        if wanted {
            let mut id: i64 = -1;
            for i in 0..count {
                let delta = i32::from_le_bytes(data[o + i * 4..o + i * 4 + 4].try_into().unwrap());
                id += 1 + delta as i64;
                let k = ids_end + i * 16;
                let key: Key = data[k..k + 16].try_into().unwrap();
                root.entry(id as u32).or_insert(key);
                if has_names {
                    let n = keys_end + i * 8;
                    let hash = u64::from_le_bytes(data[n..n + 8].try_into().unwrap());
                    names.entry(hash).or_insert(id as u32);
                }
            }
        }
        o = names_end;
    }
    let mut root: Vec<(u32, Key)> = root.into_iter().collect();
    root.sort_by_key(|(id, _)| *id);
    let mut names: Vec<(u64, u32)> = names.into_iter().collect();
    names.sort_by_key(|(h, _)| *h);
    Ok((root, names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_build_key() {
        let info = "Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|Product!STRING:0\n\
                    eu|1|aaaa|wow_classic\n\
                    eu|1|bbbb|wow\n";
        assert_eq!(build_key(info, "wow").as_deref(), Some("bbbb"));
        assert_eq!(build_key(info, "wowt"), None);
    }

    #[test]
    fn parses_index_entries() {
        let mut data = vec![0u8; 0x28];
        data[12] = 4;
        data[13] = 5;
        data[14] = 9;
        data[15] = 30;
        data[0x20..0x24].copy_from_slice(&18u32.to_le_bytes());
        let mut entry = vec![1, 2, 3, 4, 5, 6, 7, 8, 9];
        // archive 3, offset 0x100
        let packed: u64 = (3u64 << 30) | 0x100;
        entry.extend_from_slice(&packed.to_be_bytes()[3..]);
        entry.extend_from_slice(&500u32.to_le_bytes());
        data.extend(entry);
        let mut out = Vec::new();
        parse_index(&data, &mut out).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1.archive, 3);
        assert_eq!(out[0].1.offset, 0x100);
        assert_eq!(out[0].1.size, 500);
    }

    /// Reads real files when the game is installed (skipped otherwise).
    #[test]
    fn reads_local_install() {
        let dir = std::env::var("WOWPROGRAMMING_GAME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/mnt/d/World of Warcraft"));
        if !dir.join(".build.info").exists() {
            eprintln!("skipped: no game install at {}", dir.display());
            return;
        }
        let storage = Storage::open(&dir, "wow").expect("open storage");
        assert!(storage.file_count() > 100_000);
        let id = storage
            .file_id("Interface\\Icons\\INV_Misc_QuestionMark.blp")
            .expect("question mark icon");
        assert_eq!(id, 134400);
        let data = storage.read_file(id).expect("read icon");
        assert_eq!(&data[0..4], b"BLP2");
        let font = storage.read_path("Fonts\\FRIZQT__.TTF").expect("read font");
        assert!(font.len() > 10_000);
    }
}
