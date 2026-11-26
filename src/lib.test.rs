use super::*;
use std::io::Write;

fn zlib(b: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

#[test]
fn blte_of_one_chunk_and_of_several() {
    // No header: one chunk, raw.
    let mut one = b"BLTE\0\0\0\0N".to_vec();
    one.extend_from_slice(b"hello");
    assert_eq!(blte(&one).unwrap(), b"hello");
    // A header: two chunks, raw and zlib.
    let a = [b"N".as_slice(), b"abc"].concat();
    let z = [b"Z".as_slice(), &zlib(b"defdefdef")].concat();
    let header = 12 + 2 * 24;
    let mut b = b"BLTE".to_vec();
    b.extend_from_slice(&(header as u32).to_be_bytes());
    b.extend_from_slice(&[0x0F, 0, 0, 2]);
    for (c, size) in [(&a, 3u32), (&z, 9)] {
        b.extend_from_slice(&(c.len() as u32).to_be_bytes());
        b.extend_from_slice(&size.to_be_bytes());
        b.extend_from_slice(&[0; 16]);
    }
    b.extend_from_slice(&a);
    b.extend_from_slice(&z);
    assert_eq!(blte(&b).unwrap(), b"abcdefdefdef");
    // Encrypted: said.
    assert!(blte(b"BLTE\0\0\0\0Exx").unwrap_err().0.contains("'E'"));
    assert!(blte(b"nope").is_err());
}

#[test]
fn an_index_its_entries_where_they_are() {
    let mut d = Vec::new();
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    // Version 7, bucket 0, extra 0, sizes 4, 5, 9, 30 bits; max size.
    d.extend_from_slice(&[7, 0, 0, 0, 4, 5, 9, 30]);
    d.extend_from_slice(&0u64.to_le_bytes());
    d.resize(32, 0);
    d.extend_from_slice(&18u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    // Archive 3, offset 100.
    let loc = (3u64 << 30) | 100;
    d.extend_from_slice(&loc.to_be_bytes()[3..]);
    d.extend_from_slice(&500u32.to_le_bytes());
    let mut index = HashMap::new();
    read_index(&d, &mut index).unwrap();
    assert_eq!(index[&[1, 2, 3, 4, 5, 6, 7, 8, 9]], Location { archive: 3, offset: 100, size: 500 });
}

#[test]
fn the_encodings_content_keys() {
    let mut e = b"EN".to_vec();
    // Version 1, keys of 16, pages of 1 KB, one page, no espec.
    e.extend_from_slice(&[1, 16, 16, 0, 1, 0, 1]);
    e.extend_from_slice(&1u32.to_be_bytes());
    e.extend_from_slice(&0u32.to_be_bytes());
    e.push(0);
    e.extend_from_slice(&0u32.to_be_bytes());
    e.extend_from_slice(&[0; 32]);
    let page_at = e.len();
    e.push(1);
    e.extend_from_slice(&[0, 0, 0, 0, 9]);
    e.extend_from_slice(&[0xAA; 16]);
    e.extend_from_slice(&[0xBB; 16]);
    e.resize(page_at + 1024, 0);
    let map = encoding(&e).unwrap();
    assert_eq!(map[&[0xAA; 16]], [0xBB; 16]);
}

/// A Reforged install (`CASC_GAME`: the folder with `.build.info`;
/// skipped without it): its root, a model of the game's, its HD one.
#[test]
fn the_games_storage() {
    let Some(folder) = std::env::var_os("CASC_GAME") else { return };
    let s = Storage::open(Path::new(&folder)).unwrap();
    eprintln!("build {}, {} files", s.build, s.entries.len());
    assert!(s.entries.len() > 1000);
    for name in ["War3.w3mod:Units\\Human\\Footman\\Footman.mdx", "war3.w3mod:_hd.w3mod:units/human/footman/footman.mdx", "War3.w3mod:Units/UnitData.slk"] {
        assert!(s.has(name), "{name}");
        let b = s.read(name).unwrap();
        eprintln!("{name}: {} bytes", b.len());
        assert!(!b.is_empty());
    }
    assert!(s.read("War3.w3mod:Units/UnitData.slk").unwrap().starts_with(b"ID;"));
    assert!(s.read("War3.w3mod:Units/Human/Footman/Footman.mdx").unwrap().starts_with(b"MDLX"));
    assert!(s.read("War3.w3mod:nothing.txt").is_err());
}

/// Every file on disk read (`CASC_GAME`; ignored: it reads them all).
#[test]
#[ignore]
fn every_file_on_disk() {
    let Some(folder) = std::env::var_os("CASC_GAME") else { return };
    let s = Storage::open(Path::new(&folder)).unwrap();
    let (mut read, mut failed) = (0, Vec::new());
    for e in &s.entries {
        if !s.has(&e.name) {
            continue;
        }
        match s.read(&e.name) {
            Ok(_) => read += 1,
            Err(err) => failed.push(err.0),
        }
    }
    eprintln!("{read} read, {} failed", failed.len());
    for f in failed.iter().take(10) {
        eprintln!("  {f}");
    }
    assert!(failed.is_empty());
}
