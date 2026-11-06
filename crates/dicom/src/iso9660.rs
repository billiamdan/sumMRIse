//! A minimal reader for ISO 9660 disc images (ECMA-119): volume descriptors
//! and directory records, nothing else. See [`crate::media`].
//!
//! * Names come from the Joliet tree (a supplementary volume descriptor) when
//!   the image has one, else from the primary tree. The version (`;1`) and
//!   the dot of a name without extension are not part of the name.
//! * Rock Ridge names are not read: DICOM file IDs are ISO 9660 level 1
//!   names anyway.
//! * A file is read straight from its extent, or from its extents one after
//!   the other when it is recorded in several (as files larger than 4 GiB
//!   are). Files recorded in interleaved mode (a legacy of CD-i) are listed
//!   as unreadable.
//! * UDF is not read. An image that has nothing else is told apart, so that
//!   the user can be told.
//!
//! The image is untrusted: every offset is checked against the size of the
//! image, a directory is read once however often it is linked, and the depth,
//! the size of a directory and the number of files are bounded.

use std::collections::HashSet;
use std::io::{self, Read, Seek, SeekFrom};

use crate::media::{ArchiveProblem, Entry, Listing, Location, MAX_ENTRIES, MediaKind, entry_path};

/// Size of a sector, in which the volume descriptors are laid out.
const SECTOR: u64 = 2048;
/// The volume descriptors begin after the 16 sectors of the system area.
const FIRST_DESCRIPTOR: u64 = 16;
/// Most volume descriptors looked at.
const MAX_DESCRIPTORS: u64 = 64;
/// Largest directory read: room for more than 100 000 files in one folder.
const MAX_DIRECTORY_BYTES: u64 = 16 << 20;
/// Deepest folder read (ISO 9660 itself allows 8 levels, Joliet more).
const MAX_DEPTH: usize = 32;

/// File flags of a directory record (ECMA-119 9.1.6).
const FLAG_DIRECTORY: u8 = 0x02;
const FLAG_ASSOCIATED: u8 = 0x04;
const FLAG_MULTI_EXTENT: u8 = 0x80;

/// What file system a disc image has, as far as it matters here.
pub(crate) enum Volume {
    Iso9660,
    /// UDF and no ISO 9660.
    UdfOnly,
    /// Not a disc image, as far as can be told.
    None,
}

fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

fn le16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// The standard identifier of the volume descriptor in sector `index` after
/// the system area, or `None` beyond the end of the image.
fn descriptor(file: &mut (impl Read + Seek), len: u64, index: u64) -> io::Result<Option<[u8; SECTOR as usize]>> {
    let offset = (FIRST_DESCRIPTOR + index) * SECTOR;
    if offset + SECTOR > len {
        return Ok(None);
    }
    let mut sector = [0u8; SECTOR as usize];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut sector)?;
    Ok(Some(sector))
}

/// Tells an ISO 9660 image (`CD001` at byte 32769) from one with UDF only.
pub(crate) fn probe(file: &mut (impl Read + Seek), len: u64) -> io::Result<Volume> {
    let mut udf = false;
    for index in 0..MAX_DESCRIPTORS {
        let Some(sector) = descriptor(file, len, index)? else { break };
        match &sector[1..6] {
            b"CD001" => return Ok(Volume::Iso9660),
            b"NSR02" | b"NSR03" => udf = true,
            b"BEA01" | b"TEA01" | b"BOOT2" => {}
            _ => break,
        }
    }
    Ok(if udf { Volume::UdfOnly } else { Volume::None })
}

/// What a primary or supplementary volume descriptor says about its tree.
#[derive(Clone, Copy)]
struct Root {
    /// Size of a logical block.
    block: u64,
    /// The root directory.
    offset: u64,
    size: u64,
}

/// A directory still to be read.
struct Directory {
    offset: u64,
    size: u64,
    /// Names of the folders that lead to it.
    path: Vec<String>,
}

/// A file as far as its directory records have been read: every record of a
/// file in several extents but the last has the multi-extent flag, and they
/// follow each other (ECMA-119 9.1.6).
struct FileRecords {
    name: String,
    /// Offset and size of each extent, in order.
    extents: Vec<(u64, u64)>,
    problem: Option<ArchiveProblem>,
}

/// One directory record (ECMA-119 9.1).
struct Record<'a> {
    /// Length of the record.
    len: usize,
    /// First block of the content, after an extended attribute record.
    block: u64,
    size: u64,
    flags: u8,
    interleaved: bool,
    name: &'a [u8],
}

fn parse_record(bytes: &[u8]) -> Option<Record<'_>> {
    let len = usize::from(*bytes.first()?);
    if len < 34 || len > bytes.len() {
        return None;
    }
    let name_len = usize::from(bytes[32]);
    let name = bytes[..len].get(33..33 + name_len)?;
    Some(Record {
        len,
        block: u64::from(le32(&bytes[2..6])) + u64::from(bytes[1]),
        size: u64::from(le32(&bytes[10..14])),
        flags: bytes[25],
        interleaved: bytes[26] != 0,
        name,
    })
}

/// The name in a directory record: UCS-2 in the Joliet tree. The version
/// (`;1`) is not part of the name, nor is the dot of a name without
/// extension (`IM000001.;1` is the file `IM000001`).
fn decode_name(raw: &[u8], joliet: bool) -> String {
    let name = if joliet {
        let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|&pair| u16::from_be_bytes(pair)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(raw).into_owned()
    };
    let name = match name.rsplit_once(';') {
        Some((stem, version)) if !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit()) => stem,
        _ => name.as_str(),
    };
    name.strip_suffix('.').unwrap_or(name).to_owned()
}

/// Reads the directory tree of the image in `file`, which is `len` bytes long.
pub(crate) fn list(file: &mut (impl Read + Seek), len: u64) -> io::Result<Listing> {
    let mut primary: Option<Root> = None;
    let mut joliet: Option<Root> = None;
    let mut udf = false;
    for index in 0..MAX_DESCRIPTORS {
        let Some(sector) = descriptor(file, len, index)? else { break };
        match (&sector[1..6], sector[0]) {
            (b"CD001", 1) if primary.is_none() => primary = Some(volume(&sector)?),
            // Joliet marks its descriptor with one of three escape sequences.
            (b"CD001", 2) if joliet.is_none() && matches!(&sector[88..91], b"%/@" | b"%/C" | b"%/E") => {
                joliet = volume(&sector).ok();
            }
            // Boot records, partitions, further copies and the terminator of the set.
            (b"CD001", _) | (b"BEA01" | b"TEA01" | b"BOOT2", _) => {}
            (b"NSR02" | b"NSR03", _) => udf = true,
            _ => break,
        }
    }
    let is_joliet = joliet.is_some();
    let root = joliet.or(primary).ok_or_else(|| invalid("no primary volume descriptor"))?;
    let block = root.block;

    let mut entries = Vec::new();
    let mut damaged = false;
    let mut read_before = HashSet::new();
    let mut pending = vec![Directory { offset: root.offset, size: root.size, path: Vec::new() }];
    while let Some(directory) = pending.pop() {
        // A directory is read once, however often (and from wherever) it is linked.
        if !read_before.insert(directory.offset) {
            continue;
        }
        let in_image = directory.offset.checked_add(directory.size).is_some_and(|end| end <= len);
        if !in_image || directory.size > MAX_DIRECTORY_BYTES {
            damaged = true;
            continue;
        }
        let mut bytes = vec![0u8; directory.size as usize];
        file.seek(SeekFrom::Start(directory.offset))?;
        file.read_exact(&mut bytes)?;

        let mut folders = Vec::new();
        // A file whose further extents follow in the next records.
        let mut continued: Option<FileRecords> = None;
        let mut position = 0;
        while position < bytes.len() {
            if bytes[position] == 0 {
                // Records do not cross blocks: the rest of this one is empty.
                position = (position / block as usize + 1) * block as usize;
                continue;
            }
            let Some(record) = parse_record(&bytes[position..]) else {
                damaged = true;
                break;
            };
            position += record.len;
            // The directory itself and its parent.
            if matches!(record.name, [0] | [1]) {
                continue;
            }
            let name = decode_name(record.name, is_joliet);
            let names = || directory.path.iter().map(String::as_str).chain([name.as_str()]);
            if record.flags & FLAG_DIRECTORY != 0 {
                // Hidden folders are not entered, as the folder scan does not enter them.
                if directory.path.len() < MAX_DEPTH && entry_path(names()).is_some() {
                    let path = names().map(str::to_owned).collect();
                    folders.push(Directory { offset: record.block * block, size: record.size, path });
                }
                continue;
            }
            if record.flags & FLAG_ASSOCIATED != 0 {
                continue;
            }
            let mut file = match continued.take() {
                Some(file) if file.name == name => file,
                earlier => {
                    // A file in several extents whose last extent never came.
                    damaged |= earlier.is_some();
                    FileRecords { name: name.clone(), extents: Vec::new(), problem: None }
                }
            };
            let offset = record.block * block;
            if offset.checked_add(record.size).is_none_or(|end| end > len) {
                // Beyond the end of the image: it was cut off.
                file.problem = Some(ArchiveProblem::Damaged(MediaKind::Iso));
                damaged = true;
            } else if record.interleaved {
                file.problem = Some(ArchiveProblem::Damaged(MediaKind::Iso));
            }
            file.extents.push((offset, record.size));
            if record.flags & FLAG_MULTI_EXTENT != 0 {
                continued = Some(file);
                continue;
            }
            let Some(path) = entry_path(names()) else { continue };
            // At most 16 MiB of records of 34 bytes, each extent below 4 GiB: no overflow.
            let size = file.extents.iter().map(|&(_, size)| size).sum();
            entries.push(Entry::new(path, size, Location::Extents(file.extents), file.problem));
            if entries.len() > MAX_ENTRIES {
                return Err(invalid("the disc image lists too many files"));
            }
        }
        damaged |= continued.is_some();
        // Depth first, in the order of the directory.
        pending.extend(folders.into_iter().rev());
    }
    if damaged && entries.is_empty() {
        return Err(invalid("the directory of the disc image cannot be read"));
    }
    Ok(Listing { entries, zip: None, udf, damaged })
}

/// Block size and root directory of a primary or supplementary volume
/// descriptor (ECMA-119 8.4).
fn volume(sector: &[u8; SECTOR as usize]) -> io::Result<Root> {
    let block = u64::from(le16(&sector[128..130]));
    if !block.is_power_of_two() || block < 512 {
        return Err(invalid("implausible logical block size"));
    }
    let root = parse_record(&sector[156..190]).ok_or_else(|| invalid("no root directory record"))?;
    Ok(Root { block, offset: root.block * block, size: root.size })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_version_and_trailing_dot() {
        assert_eq!(decode_name(b"IM000001.;1", false), "IM000001");
        assert_eq!(decode_name(b"README.TXT;1", false), "README.TXT");
        assert_eq!(decode_name(b"DICOM", false), "DICOM");
        assert_eq!(decode_name(b"A;B", false), "A;B", "not a version");
        let joliet: Vec<u8> = "Röntgen.dcm;1".encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(decode_name(&joliet, true), "Röntgen.dcm");
    }

    #[test]
    fn malformed_records_are_not_records() {
        assert!(parse_record(&[]).is_none());
        assert!(parse_record(&[0; 40]).is_none(), "length 0");
        assert!(parse_record(&[20; 40]).is_none(), "shorter than a record can be");
        let mut cut = [0u8; 40];
        cut[0] = 60;
        assert!(parse_record(&cut).is_none(), "longer than what is left");
        let mut long_name = [0u8; 40];
        (long_name[0], long_name[32]) = (36, 10);
        assert!(parse_record(&long_name).is_none(), "the name does not fit into the record");
        let mut file = [0u8; 40];
        (file[0], file[2], file[10], file[32]) = (36, 7, 9, 2);
        file[33..35].copy_from_slice(b"A.");
        let record = parse_record(&file).expect("a record");
        assert_eq!((record.len, record.block, record.size, record.name), (36, 7, 9, b"A.".as_slice()));
    }
}
