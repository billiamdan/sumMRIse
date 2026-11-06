//! Archives and disc images of synthetic studies, for the tests of opening
//! them: a ZIP archive and an ISO 9660 image, written here from the format
//! descriptions (APPNOTE.TXT, ECMA-119, Joliet) and so independently of the
//! readers under test.
//!
//! [`zip_folder`] and [`iso_image`] pack a folder — usually one written by
//! [`crate::write_phantom_study`] — the way such media reach patients;
//! [`zip_archive`] builds an archive entry by entry, also in ways no honest
//! tool would (see [`ZipEntry`]).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::FixtureError;

/// Files and folders below `root` as paths relative to it, each folder
/// before what it holds, sorted by name. `true` marks a folder.
fn walk(root: &Path) -> Result<Vec<(PathBuf, bool)>, FixtureError> {
    fn visit(dir: &Path, relative: &Path, out: &mut Vec<(PathBuf, bool)>) -> Result<(), FixtureError> {
        let mut names: Vec<_> = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
        names.sort_by_key(fs::DirEntry::file_name);
        for entry in names {
            let path = relative.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                out.push((path.clone(), true));
                visit(&entry.path(), &path, out)?;
            } else {
                out.push((path, false));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(root, Path::new(""), &mut out)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// ZIP

/// How an entry of a synthetic ZIP archive is compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZipMethod {
    Stored,
    Deflated,
    /// Method 9, which Windows uses for big files. The data is written as a
    /// deflate stream of stored blocks, which is valid deflate64 as well.
    Deflate64,
    /// Any other method number (12 is bzip2, 14 LZMA, 93 Zstandard); the data
    /// is written as it is.
    Other(u16),
}

/// One entry of a synthetic ZIP archive.
#[derive(Debug, Clone)]
pub struct ZipEntry {
    /// Path in the archive, with `/` between folders; a folder ends in `/`.
    pub name: String,
    pub data: Vec<u8>,
    pub method: ZipMethod,
    /// Marks the entry as encrypted and scrambles its data, as if a password
    /// were needed.
    pub encrypted: bool,
    /// Uncompressed size to declare instead of the real one.
    pub declared_size: Option<u64>,
    /// Sizes and checksum follow the data in a data descriptor, as tools that
    /// write an archive in one pass store them.
    pub streamed: bool,
}

impl ZipEntry {
    pub fn new(name: impl Into<String>, data: impl Into<Vec<u8>>, method: ZipMethod) -> Self {
        Self { name: name.into(), data: data.into(), method, encrypted: false, declared_size: None, streamed: false }
    }
}

/// CRC-32 as ZIP uses it (IEEE 802.3).
fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut value = i as u32;
            let mut bit = 0;
            while bit < 8 {
                value = if value & 1 == 1 { (value >> 1) ^ 0xEDB8_8320 } else { value >> 1 };
                bit += 1;
            }
            table[i] = value;
            i += 1;
        }
        table
    };
    !data.iter().fold(!0u32, |crc, &byte| TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8))
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    // Writing to a Vec cannot fail.
    encoder.write_all(data).and_then(|()| encoder.finish()).unwrap_or_default()
}

/// A deflate stream made of stored blocks only.
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65_535 * 5 + 5);
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    let mut blocks = data.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        let len = block.len() as u16;
        // Final-block bit, block type 00.
        out.push(u8::from(blocks.peek().is_none()));
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out
}

/// 2026-01-15 09:30:00 as an MS-DOS date and time.
const DOS_DATE: u16 = ((2026 - 1980) << 9) | (1 << 5) | 15;
const DOS_TIME: u16 = (9 << 11) | (30 << 5);

/// A ZIP archive with `entries`, in that order.
pub fn zip_archive(entries: &[ZipEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for entry in entries {
        let name = entry.name.as_bytes();
        let is_folder = entry.name.ends_with('/');
        let crc = crc32(&entry.data);
        let (method, mut data) = match entry.method {
            ZipMethod::Stored => (0, entry.data.clone()),
            ZipMethod::Deflated => (8, deflate(&entry.data)),
            ZipMethod::Deflate64 => (9, deflate_stored(&entry.data)),
            ZipMethod::Other(method) => (method, entry.data.clone()),
        };
        // Bit 11: the name is UTF-8.
        let mut flags = 0x0800u16;
        if entry.encrypted {
            flags |= 0x0001;
            // The 12-byte encryption header, then data that only a password would make sense of.
            data = std::iter::repeat_n(0x5A, 12).chain(data.iter().map(|byte| byte ^ 0xA5)).collect();
        }
        if entry.streamed {
            flags |= 0x0008;
        }
        let size = entry.declared_size.unwrap_or(entry.data.len() as u64);
        let compressed = data.len() as u64;
        // A size beyond 32 bits goes into a ZIP64 extra field.
        let zip64 = size > u64::from(u32::MAX);
        let size32 = if zip64 { u32::MAX } else { size as u32 };
        let compressed32 = if zip64 { u32::MAX } else { compressed as u32 };
        let version = if zip64 { 45u16 } else { 20 };
        let offset = out.len() as u32;

        // Local file header.
        let in_header = !entry.streamed;
        let local_extra: Vec<u8> = if zip64 {
            [&1u16.to_le_bytes()[..], &16u16.to_le_bytes(), &size.to_le_bytes(), &compressed.to_le_bytes()].concat()
        } else {
            Vec::new()
        };
        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&DOS_TIME.to_le_bytes());
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&(if in_header { crc } else { 0 }).to_le_bytes());
        out.extend_from_slice(&(if in_header { compressed32 } else { 0 }).to_le_bytes());
        out.extend_from_slice(&(if in_header { size32 } else { 0 }).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(local_extra.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&local_extra);
        out.extend_from_slice(&data);
        if entry.streamed {
            out.extend_from_slice(&0x0807_4B50u32.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&compressed32.to_le_bytes());
            out.extend_from_slice(&size32.to_le_bytes());
        }

        // Central directory header.
        let extra: Vec<u8> = if zip64 {
            [&1u16.to_le_bytes()[..], &16u16.to_le_bytes(), &size.to_le_bytes(), &compressed.to_le_bytes()].concat()
        } else {
            Vec::new()
        };
        // Unix permissions: a regular file or a folder.
        let attributes: u32 = if is_folder { (0o040_755 << 16) | 0x10 } else { 0o100_644 << 16 };
        directory.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        // Made on Unix (3), by version 2.0 or 4.5.
        directory.extend_from_slice(&((3u16 << 8) | version).to_le_bytes());
        directory.extend_from_slice(&version.to_le_bytes());
        directory.extend_from_slice(&flags.to_le_bytes());
        directory.extend_from_slice(&method.to_le_bytes());
        directory.extend_from_slice(&DOS_TIME.to_le_bytes());
        directory.extend_from_slice(&DOS_DATE.to_le_bytes());
        directory.extend_from_slice(&crc.to_le_bytes());
        directory.extend_from_slice(&compressed32.to_le_bytes());
        directory.extend_from_slice(&size32.to_le_bytes());
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        directory.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        // No comment, first disk, no internal attributes.
        directory.extend_from_slice(&[0; 6]);
        directory.extend_from_slice(&attributes.to_le_bytes());
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name);
        directory.extend_from_slice(&extra);
    }
    let directory_offset = out.len() as u32;
    out.extend_from_slice(&directory);
    // End of central directory record.
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&directory_offset.to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    out
}

/// Packs the folder `root` into a ZIP archive the way a download from a
/// hospital portal, or a disc compressed in the Finder, looks: everything
/// below one top-level folder `top`, folders as entries of their own, files
/// stored and deflated in turn (every third with a data descriptor), and the
/// `__MACOSX` folder and `.DS_Store` of archives made on a Mac.
pub fn zip_folder(root: &Path, top: &str) -> Result<Vec<u8>, FixtureError> {
    let mut entries = vec![ZipEntry::new(format!("{top}/"), [], ZipMethod::Stored)];
    let mut clutter = Vec::new();
    let mut files = 0;
    for (relative, is_folder) in walk(root)? {
        let name: Vec<String> = relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        if is_folder {
            entries.push(ZipEntry::new(format!("{top}/{}/", name.join("/")), [], ZipMethod::Stored));
            continue;
        }
        let method = if files % 2 == 0 { ZipMethod::Stored } else { ZipMethod::Deflated };
        let mut entry = ZipEntry::new(format!("{top}/{}", name.join("/")), fs::read(root.join(&relative))?, method);
        entry.streamed = files % 3 == 2;
        entries.push(entry);
        files += 1;
        // The resource fork the Finder adds for every file (an AppleDouble header).
        let (file, folders) = name.split_last().map_or(("", &[][..]), |(file, folders)| (file.as_str(), folders));
        let folder: String = folders.iter().map(|folder| format!("{folder}/")).collect();
        let fork = [&[0x00, 0x05, 0x16, 0x07, 0x00, 0x02, 0x00, 0x00][..], &[0u8; 200]].concat();
        clutter.push(ZipEntry::new(format!("__MACOSX/{top}/{folder}._{file}"), fork, ZipMethod::Deflated));
    }
    entries.push(ZipEntry::new(format!("{top}/.DS_Store"), vec![0x42u8; 300], ZipMethod::Deflated));
    entries.extend(clutter);
    Ok(zip_archive(&entries))
}

// ---------------------------------------------------------------------------
// ISO 9660

/// How the files of a synthetic disc image are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsoNames {
    /// ISO 9660 level 1 only: upper-case 8.3 names with a version, such as
    /// `IM000001.;1` and `README.TXT;1`.
    Level1,
    /// Level 1 names, and a Joliet tree with the names as they are in the
    /// folder.
    Joliet,
}

/// What [`iso_image_with`] writes.
#[derive(Debug, Clone)]
pub struct IsoOptions {
    pub names: IsoNames,
    /// Name of a file to record in two extents, the way files larger than
    /// 4 GiB are recorded. The second part lies before the first on the
    /// image, so that only a reader that follows the extents gets it right.
    pub in_two_extents: Option<String>,
}

const SECTOR: usize = 2048;
/// The two trees of an image: primary (level 1 names) and Joliet.
const PRIMARY: usize = 0;
const JOLIET: usize = 1;

struct IsoFolder {
    /// Index of the parent folder (the root is its own parent).
    parent: usize,
    /// Identifier in the primary and in the Joliet tree.
    names: [Vec<u8>; 2],
    folders: Vec<usize>,
    files: Vec<IsoFile>,
    /// First sector and size in bytes of the directory, per tree.
    extent: [(u32, u32); 2],
}

struct IsoFile {
    names: [Vec<u8>; 2],
    data: Vec<u8>,
    sector: u32,
    in_two_extents: bool,
}

impl IsoFile {
    /// Size of the first extent of a file recorded in two (a whole number of
    /// sectors), or `None` for a file in one extent.
    fn first_extent(&self) -> Option<usize> {
        let first = (self.data.len() / 2).next_multiple_of(SECTOR);
        (self.in_two_extents && first < self.data.len()).then_some(first)
    }
}

fn ucs2(name: &str) -> Vec<u8> {
    name.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// An upper-case 8.3 name, or an error if `name` does not fit into one.
fn level1(name: &str, is_folder: bool) -> Result<Vec<u8>, FixtureError> {
    let upper = name.to_ascii_uppercase();
    let (stem, extension) =
        if is_folder { (upper.as_str(), "") } else { upper.rsplit_once('.').unwrap_or((&upper, "")) };
    let allowed = |part: &str| part.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    if stem.is_empty() || stem.len() > 8 || extension.len() > 3 || !allowed(stem) || !allowed(extension) {
        return Err(FixtureError::Dicom(format!("{name} is not an ISO 9660 level 1 name")));
    }
    Ok(if is_folder { upper.into_bytes() } else { format!("{stem}.{extension};1").into_bytes() })
}

fn both_endian_32(value: u32) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&value.to_le_bytes());
    out[4..].copy_from_slice(&value.to_be_bytes());
    out
}

fn both_endian_16(value: u16) -> [u8; 4] {
    let mut out = [0u8; 4];
    out[..2].copy_from_slice(&value.to_le_bytes());
    out[2..].copy_from_slice(&value.to_be_bytes());
    out
}

/// A directory record (ECMA-119 9.1) for the extent at `sector`.
fn directory_record(name: &[u8], sector: u32, size: u32, flags: u8) -> Vec<u8> {
    // A padding byte keeps the length of the record even.
    let len = 33 + name.len() + usize::from(name.len().is_multiple_of(2));
    let mut record = vec![0u8; len];
    record[0] = len as u8;
    record[2..10].copy_from_slice(&both_endian_32(sector));
    record[10..18].copy_from_slice(&both_endian_32(size));
    // Recorded on 2026-01-15 at 09:30:00 UTC.
    record[18..25].copy_from_slice(&[126, 1, 15, 9, 30, 0, 0]);
    record[25] = flags;
    record[28..32].copy_from_slice(&both_endian_16(1));
    record[32] = name.len() as u8;
    record[33..33 + name.len()].copy_from_slice(name);
    record
}

const FLAG_FOLDER: u8 = 0x02;
const FLAG_MULTI_EXTENT: u8 = 0x80;

/// The directory of folder `index` in tree `tree`: records never cross a
/// sector, and the directory fills whole sectors.
fn directory(folders: &[IsoFolder], index: usize, tree: usize) -> Vec<u8> {
    let folder = &folders[index];
    let parent = &folders[folder.parent];
    let mut records = vec![
        directory_record(&[0], folder.extent[tree].0, folder.extent[tree].1, FLAG_FOLDER),
        directory_record(&[1], parent.extent[tree].0, parent.extent[tree].1, FLAG_FOLDER),
    ];
    let mut named: Vec<(&[u8], Vec<Vec<u8>>)> = Vec::new();
    for &child in &folder.folders {
        let child = &folders[child];
        let name = child.names[tree].as_slice();
        named.push((name, vec![directory_record(name, child.extent[tree].0, child.extent[tree].1, FLAG_FOLDER)]));
    }
    for file in &folder.files {
        let name = file.names[tree].as_slice();
        let size = file.data.len() as u32;
        let records = match file.first_extent() {
            Some(first) => {
                // The second part is written first (see `iso_image_with`).
                let (first, second) = (first as u32, size - first as u32);
                let second_sectors = second.div_ceil(SECTOR as u32);
                vec![
                    directory_record(name, file.sector + second_sectors, first, FLAG_MULTI_EXTENT),
                    directory_record(name, file.sector, second, 0),
                ]
            }
            None => vec![directory_record(name, file.sector, size, 0)],
        };
        named.push((name, records));
    }
    named.sort_by(|a, b| a.0.cmp(b.0));
    records.extend(named.into_iter().flat_map(|(_, records)| records));

    let mut out = Vec::new();
    for record in records {
        let used = out.len() % SECTOR;
        if used + record.len() > SECTOR {
            out.resize(out.len() + SECTOR - used, 0);
        }
        out.extend_from_slice(&record);
    }
    out.resize(out.len().div_ceil(SECTOR) * SECTOR, 0);
    out
}

/// The path table of tree `tree` (ECMA-119 9.4), little or big endian.
fn path_table(folders: &[IsoFolder], tree: usize, big_endian: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, folder) in folders.iter().enumerate() {
        let name: &[u8] = if index == 0 { &[0] } else { &folder.names[tree] };
        let (sector, parent) = (folder.extent[tree].0, folder.parent as u16 + 1);
        out.extend_from_slice(&[name.len() as u8, 0]);
        out.extend_from_slice(&if big_endian { sector.to_be_bytes() } else { sector.to_le_bytes() });
        out.extend_from_slice(&if big_endian { parent.to_be_bytes() } else { parent.to_le_bytes() });
        out.extend_from_slice(name);
        if name.len() % 2 == 1 {
            out.push(0);
        }
    }
    out
}

/// `text` padded with spaces (or UCS-2 spaces) to `len` bytes.
fn padded(text: &str, len: usize, tree: usize) -> Vec<u8> {
    let mut out = if tree == JOLIET { ucs2(text) } else { text.as_bytes().to_vec() };
    while out.len() < len {
        out.extend_from_slice(if tree == JOLIET { &[0, b' '] } else { b" " });
    }
    out.truncate(len);
    out
}

/// A primary or supplementary (Joliet) volume descriptor (ECMA-119 8.4, 8.5).
fn volume_descriptor(folders: &[IsoFolder], tree: usize, sectors: u32, tables: (u32, u32, u32)) -> Vec<u8> {
    let (table_size, little_endian_table, big_endian_table) = tables;
    let mut d = vec![0u8; SECTOR];
    d[0] = if tree == JOLIET { 2 } else { 1 };
    d[1..6].copy_from_slice(b"CD001");
    d[6] = 1;
    d[8..40].copy_from_slice(&padded("SUMMRISE", 32, tree));
    d[40..72].copy_from_slice(&padded("SUMMRISE_TEST", 32, tree));
    d[80..88].copy_from_slice(&both_endian_32(sectors));
    if tree == JOLIET {
        // UCS-2 level 3.
        d[88..91].copy_from_slice(b"%/E");
    }
    d[120..124].copy_from_slice(&both_endian_16(1));
    d[124..128].copy_from_slice(&both_endian_16(1));
    d[128..132].copy_from_slice(&both_endian_16(SECTOR as u16));
    d[132..140].copy_from_slice(&both_endian_32(table_size));
    d[140..144].copy_from_slice(&little_endian_table.to_le_bytes());
    d[148..152].copy_from_slice(&big_endian_table.to_be_bytes());
    let root = directory_record(&[0], folders[0].extent[tree].0, folders[0].extent[tree].1, FLAG_FOLDER);
    d[156..190].copy_from_slice(&root);
    for field in [(190, 128), (318, 128), (446, 128), (574, 128), (702, 37), (739, 37), (776, 37)] {
        d[field.0..field.0 + field.1].copy_from_slice(&padded("", field.1, tree));
    }
    // Created and modified on 2026-01-15 at 09:30:00; never expires.
    for (at, date) in
        [(813, b"2026011509300000"), (830, b"2026011509300000"), (847, b"0000000000000000"), (864, b"0000000000000000")]
    {
        d[at..at + 16].copy_from_slice(date);
    }
    d[881] = 1;
    d
}

/// Writes the folder `root` as an ISO 9660 disc image.
pub fn iso_image(root: &Path, names: IsoNames) -> Result<Vec<u8>, FixtureError> {
    iso_image_with(root, &IsoOptions { names, in_two_extents: None })
}

/// Writes the folder `root` as an ISO 9660 disc image: system area, volume
/// descriptors, path tables, the directories of the primary tree (and of the
/// Joliet tree, pointing at the same files), then the files.
pub fn iso_image_with(root: &Path, options: &IsoOptions) -> Result<Vec<u8>, FixtureError> {
    // The folders in the order of the path table: level by level, each parent
    // before its children, sorted by name.
    let folder = |parent: usize, names: [Vec<u8>; 2]| IsoFolder {
        parent,
        names,
        folders: Vec::new(),
        files: Vec::new(),
        extent: [(0, 0); 2],
    };
    let mut folders = vec![folder(0, [vec![0], vec![0]])];
    let mut paths = vec![root.to_path_buf()];
    let mut index = 0;
    while index < folders.len() {
        let mut entries = fs::read_dir(&paths[index])?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type()?.is_dir() {
                let child = folders.len();
                folders.push(folder(index, [level1(&name, true)?, ucs2(&name)]));
                folders[index].folders.push(child);
                paths.push(entry.path());
            } else {
                folders[index].files.push(IsoFile {
                    names: [level1(&name, false)?, ucs2(&format!("{name};1"))],
                    data: fs::read(entry.path())?,
                    sector: 0,
                    in_two_extents: options.in_two_extents.as_deref() == Some(name.as_str()),
                });
            }
        }
        index += 1;
    }
    let trees: &[usize] = if options.names == IsoNames::Joliet { &[PRIMARY, JOLIET] } else { &[PRIMARY] };

    // Lay the image out: descriptors from sector 16, the terminator, then the rest.
    let sectors_of = |bytes: usize| bytes.div_ceil(SECTOR) as u32;
    let mut next = 16 + trees.len() as u32 + 1;
    let mut tables = [(0, 0, 0); 2];
    for &tree in trees {
        let size = path_table(&folders, tree, false).len();
        tables[tree] = (size as u32, next, next + sectors_of(size));
        next += 2 * sectors_of(size);
    }
    for &tree in trees {
        for index in 0..folders.len() {
            // The size of a directory does not depend on where things are.
            let size = directory(&folders, index, tree).len();
            folders[index].extent[tree] = (next, size as u32);
            next += sectors_of(size);
        }
    }
    for folder in &mut folders {
        for file in &mut folder.files {
            file.sector = next;
            next += sectors_of(file.data.len());
        }
    }
    let total = next;

    let mut image = vec![0u8; 16 * SECTOR];
    for &tree in trees {
        image.extend_from_slice(&volume_descriptor(&folders, tree, total, tables[tree]));
    }
    let mut terminator = vec![0u8; SECTOR];
    terminator[0] = 255;
    terminator[1..6].copy_from_slice(b"CD001");
    terminator[6] = 1;
    image.extend_from_slice(&terminator);
    let pad = |image: &mut Vec<u8>| image.resize(image.len().div_ceil(SECTOR) * SECTOR, 0);
    for &tree in trees {
        for big_endian in [false, true] {
            image.extend_from_slice(&path_table(&folders, tree, big_endian));
            pad(&mut image);
        }
    }
    for &tree in trees {
        for index in 0..folders.len() {
            image.extend_from_slice(&directory(&folders, index, tree));
        }
    }
    for folder in &folders {
        for file in &folder.files {
            match file.first_extent() {
                // The same sectors as in one piece: the first part is a whole number of them.
                Some(first) => {
                    image.extend_from_slice(&file.data[first..]);
                    pad(&mut image);
                    image.extend_from_slice(&file.data[..first]);
                }
                None => image.extend_from_slice(&file.data),
            }
            pad(&mut image);
        }
    }
    debug_assert_eq!(image.len(), total as usize * SECTOR);
    Ok(image)
}

/// A disc image with a UDF file system and no ISO 9660 one (only the volume
/// recognition sequence is there, which is all that matters for telling).
pub fn udf_only_image() -> Vec<u8> {
    let mut image = vec![0u8; 32 * SECTOR];
    for (sector, identifier) in [(16, b"BEA01"), (17, b"NSR02"), (18, b"TEA01")] {
        let at = sector * SECTOR;
        image[at + 1..at + 6].copy_from_slice(identifier);
        image[at + 6] = 1;
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_reference_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn stored_blocks_cover_the_data() {
        assert_eq!(deflate_stored(b""), [1, 0, 0, 0xFF, 0xFF]);
        assert_eq!(deflate_stored(b"ab"), [1, 2, 0, 0xFD, 0xFF, b'a', b'b']);
        let long = deflate_stored(&vec![7u8; 70_000]);
        assert_eq!(long.len(), 70_000 + 2 * 5);
        assert_eq!((long[0], long[65_540]), (0, 1), "only the last block is final");
    }

    #[test]
    fn level1_names_are_upper_case_with_a_version() {
        assert_eq!(level1("IM000001", false).expect("name"), b"IM000001.;1");
        assert_eq!(level1("viewer.exe", false).expect("name"), b"VIEWER.EXE;1");
        assert_eq!(level1("ST000001", true).expect("name"), b"ST000001");
        assert!(level1("a much too long name.dcm", false).is_err());
    }

    #[test]
    fn zip_archive_has_one_directory_entry_per_entry() {
        let entries = [
            ZipEntry::new("CD/", [], ZipMethod::Stored),
            ZipEntry::new("CD/A", b"alpha".as_slice(), ZipMethod::Stored),
            ZipEntry::new("CD/B", vec![1u8; 4000], ZipMethod::Deflated),
        ];
        let zip = zip_archive(&entries);
        assert!(zip.starts_with(b"PK\x03\x04"));
        let end = &zip[zip.len() - 22..];
        assert_eq!(&end[..4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([end[10], end[11]]), 3);
        assert!(zip.len() < 1000, "the second file is compressed: {} bytes", zip.len());
        assert_eq!(zip.windows(4).filter(|w| w == b"PK\x01\x02").count(), 3);
    }

    /// The synthetic CD in a folder `CD` of a temporary folder (which is returned).
    fn synthetic_cd(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("summrise-testkit-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        crate::write_phantom_study(&dir.join("CD"), &crate::PhantomOptions::small()).expect("fixture");
        dir
    }

    /// Output of `tool` run in `dir`, or `None` if the tool is not installed.
    fn output_of(tool: &str, args: &[&str], dir: &Path) -> Option<String> {
        match std::process::Command::new(tool).args(args).current_dir(dir).output() {
            Ok(output) => {
                assert!(output.status.success(), "{tool} {args:?}: {}", String::from_utf8_lossy(&output.stderr));
                Some(String::from_utf8_lossy(&output.stdout).into_owned())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => panic!("{tool} could not be started: {e}"),
        }
    }

    /// The archives written here are checked by a reader that has nothing to
    /// do with the ones under test: Info-ZIP verifies every checksum.
    #[test]
    fn zip_of_the_synthetic_cd_passes_the_test_of_unzip() {
        let dir = synthetic_cd("zip");
        fs::write(dir.join("study.zip"), zip_folder(&dir.join("CD"), "PHANTOM CD").expect("zip")).expect("write");
        if let Some(listing) = output_of("unzip", &["-t", "study.zip"], &dir) {
            assert!(listing.contains("No errors detected"), "{listing}");
            assert!(listing.contains("PHANTOM CD/DICOM/ST000001/SE000001/IM000024"), "{listing}");
            assert!(listing.contains("__MACOSX/PHANTOM CD/DICOM/ST000001/SE000001/._IM000024"), "{listing}");
        }
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// libarchive (the `tar` of macOS and of many Linux systems) reads ISO
    /// 9660 images: it finds every file of the images written here, and the
    /// content it extracts is the content of the file.
    #[test]
    fn iso_of_the_synthetic_cd_is_read_by_libarchive() {
        let dir = synthetic_cd("iso");
        let expected = fs::read(dir.join("CD/DICOM/ST000001/SE000002/IM000007")).expect("read");
        for (name, names) in [("level1.iso", IsoNames::Level1), ("joliet.iso", IsoNames::Joliet)] {
            fs::write(dir.join(name), iso_image(&dir.join("CD"), names).expect("iso")).expect("write");
            let Some(listing) = output_of("tar", &["-tf", name], &dir) else { break };
            let listed: Vec<String> = listing.lines().map(|line| line.trim_end_matches('/').to_uppercase()).collect();
            for file in ["DICOMDIR", "AUTORUN.INF", "VIEWER/VIEWER.EXE", "DICOM/ST000001/SE000099/IM000001"] {
                assert!(listed.iter().any(|line| line == file), "{name}: {file} is missing from {listed:?}");
            }
            assert_eq!(listed.iter().filter(|line| line.contains("/IM0000")).count(), 40, "{name}");
            // Extract one file to a pipe (nothing is written) and compare.
            let member =
                listing.lines().find(|line| line.to_uppercase().ends_with("SE000002/IM000007")).expect("listed");
            let output = std::process::Command::new("tar").args(["-xOf", name, member]).current_dir(&dir).output();
            assert_eq!(output.expect("tar").stdout, expected, "{name}");
        }
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
