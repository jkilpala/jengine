//! The `.jpak` archive a pack is shipped as: a header, a JSON index of
//! `(path, offset, len)`, then every file's bytes back to back, stored as
//! they are. Uncompressed on purpose - what a pack holds is mostly PNG, OGG
//! and MP4, already compressed - so a browser can slice any file straight
//! out of the downloaded buffer without unpacking anything into wasm memory.
//!
//! ```text
//! b"JPAK" | version u8 (1) | 3 zero bytes | index length u32 LE | index JSON | data
//! ```
//!
//! Offsets are from the start of the data. A path is checked before it is
//! used ([`safe_path`]): an archive comes off the network, and one naming
//! `../../x` must not write outside its folder.

use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8; 4] = b"JPAK";
pub const VERSION: u8 = 1;
/// The fixed part before the index.
pub const HEADER_LEN: usize = 12;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub files: Vec<IndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub path: String,
    pub offset: u64,
    pub len: u64,
}

/// Whether `path` is a plain relative path with no way out of its folder.
pub fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.contains('\0')
        && path.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// The index for `files` (path, length), in that order.
pub fn index_for(files: &[(String, u64)]) -> Index {
    let mut offset = 0;
    let files = files
        .iter()
        .map(|(path, len)| {
            let e = IndexEntry { path: path.clone(), offset, len: *len };
            offset += len;
            e
        })
        .collect();
    Index { files }
}

/// The header and index for `index`, to be followed by the data.
pub fn header(index: &Index) -> Vec<u8> {
    let json = serde_json::to_vec(index).expect("an index serialises");
    let mut out = Vec::with_capacity(HEADER_LEN + json.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&[0, 0, 0]);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out
}

/// A whole archive in memory, for tests and small packs.
pub fn encode(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let sizes: Vec<(String, u64)> = files.iter().map(|(p, b)| (p.clone(), b.len() as u64)).collect();
    let mut out = header(&index_for(&sizes));
    for (_, b) in files {
        out.extend_from_slice(b);
    }
    out
}

/// How many bytes the header and index take, from the first [`HEADER_LEN`].
pub fn index_end(head: &[u8]) -> anyhow::Result<usize> {
    anyhow::ensure!(head.len() >= HEADER_LEN, "not a pack archive (too short)");
    anyhow::ensure!(&head[..4] == MAGIC, "not a pack archive (no JPAK header)");
    anyhow::ensure!(head[4] == VERSION, "pack archive version {} (this engine reads {VERSION})", head[4]);
    let n = u32::from_le_bytes([head[8], head[9], head[10], head[11]]) as usize;
    Ok(HEADER_LEN + n)
}

/// The index, from the archive's first [`index_end`] bytes (or more), with
/// every path checked and every range inside `total` bytes. Returns it and
/// where the data starts.
pub fn read_index(bytes: &[u8], total: u64) -> anyhow::Result<(Index, u64)> {
    let end = index_end(bytes)?;
    anyhow::ensure!(bytes.len() >= end, "pack archive index cut short");
    let index: Index = serde_json::from_slice(&bytes[HEADER_LEN..end])?;
    let data = end as u64;
    for f in &index.files {
        anyhow::ensure!(safe_path(&f.path), "pack archive names an unsafe path '{}'", f.path);
        let stop = data.checked_add(f.offset).and_then(|s| s.checked_add(f.len));
        anyhow::ensure!(stop.is_some_and(|s| s <= total), "pack archive entry '{}' runs past its end", f.path);
    }
    Ok((index, data))
}

/// Unpack the archive at `archive` into `dest`, returning the paths written.
#[cfg(not(target_arch = "wasm32"))]
pub fn extract(archive: &std::path::Path, dest: &std::path::Path) -> anyhow::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(archive)?;
    let total = file.metadata()?.len();
    let mut head = [0u8; HEADER_LEN];
    file.read_exact(&mut head)?;
    let end = index_end(&head)?;
    let mut bytes = head.to_vec();
    bytes.resize(end, 0);
    file.read_exact(&mut bytes[HEADER_LEN..])?;
    let (index, data) = read_index(&bytes, total)?;
    let mut written = Vec::with_capacity(index.files.len());
    for f in &index.files {
        let out = dest.join(&f.path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        file.seek(SeekFrom::Start(data + f.offset))?;
        let mut w = std::fs::File::create(&out)?;
        let copied = std::io::copy(&mut (&mut file).take(f.len), &mut w)?;
        anyhow::ensure!(copied == f.len, "pack archive entry '{}' cut short", f.path);
        written.push(f.path.clone());
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_round_trips() {
        let files = vec![("sprites/a.png".to_string(), vec![1, 2, 3]), ("scenes/b.scene".to_string(), b"{}".to_vec())];
        let bytes = encode(&files);
        let (index, data) = read_index(&bytes, bytes.len() as u64).unwrap();
        assert_eq!(index.files.len(), 2);
        let b = &index.files[1];
        let s = (data + b.offset) as usize;
        assert_eq!(&bytes[s..s + b.len as usize], b"{}");
    }

    #[test]
    fn a_path_out_of_the_folder_is_refused() {
        for bad in ["../x", "a/../../x", "/etc/x", "C:/x", "a\\b", "", "a//b", "./a"] {
            assert!(!safe_path(bad), "{bad:?}");
        }
        assert!(safe_path("sprites/forest/tree.png"));
        let bytes = encode(&[("../evil".to_string(), vec![0])]);
        assert!(read_index(&bytes, bytes.len() as u64).is_err());
    }

    #[test]
    fn a_short_or_foreign_file_is_refused() {
        assert!(index_end(b"PK\x03\x04\0\0\0\0\0\0\0\0").is_err());
        let mut bytes = encode(&[("a.png".to_string(), vec![0; 10])]);
        bytes.truncate(bytes.len() - 5);
        assert!(read_index(&bytes, bytes.len() as u64).is_err(), "an entry past the end");
    }

    #[test]
    fn extract_writes_every_file() {
        let dir = std::env::temp_dir().join(format!("jengine-jpak-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("p.jpak");
        std::fs::write(&archive, encode(&[("music/a.ogg".to_string(), vec![7; 100]), ("b.txt".to_string(), b"hi".to_vec())])).unwrap();
        let out = dir.join("out");
        let written = extract(&archive, &out).unwrap();
        assert_eq!(written, vec!["music/a.ogg", "b.txt"]);
        assert_eq!(std::fs::read(out.join("music/a.ogg")).unwrap(), vec![7; 100]);
        assert_eq!(std::fs::read(out.join("b.txt")).unwrap(), b"hi");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
