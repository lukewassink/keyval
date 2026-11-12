use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;

use crate::storage::StorageError;

// Pahther DB magic number: 0PNTH\9e\fa\b4
const MAGIC_NO: &[u8] = &[0x00, 0x70, 0x6E, 0x74, 0x68, 0x9e, 0xfa, 0xb4];
const EXTENSION: &str = "data";
const FILENAME_ROOT: &str = "pantherdb_data_";
// Hardcode version to 0 for now.
// I can allow configuring it later if the storage format evolves.
const VERSION: u32 = 0;
const HEADER_LENGTH: usize = 16;

fn open_file(path: &str, create: bool) -> Result<File, StorageError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(path)
        .map_err(Into::into)
}

struct Header {
    format_version: u32,
    file_no: u32,
}

pub(crate) fn parse_u32(bytes: &[u8], offset: u64) -> u32 {
    let offset = offset as usize;
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

pub(crate) fn parse_u64(bytes: &[u8], offset: u64) -> u64 {
    let offset = offset as usize;
    u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

/// Header format:
/// [magic number - bytes][format version: 4 bytes][file number - 4 bytes]
///
/// parse_header() attempts to extract header data from file assuming the above format.
/// It will return None if:
/// - it does not have permissithe to read the file
/// - the file does not begin with the magic number
/// - the file is shorter than the header length.
fn parse_header(file: &File) -> Result<Option<Header>, StorageError> {
    let mut buf: [u8; HEADER_LENGTH] = [0; HEADER_LENGTH];

    if let Err(e) = file.read_exact_at(&mut buf, 0) {
        if e.kind() == io::ErrorKind::UnexpectedEof || e.kind() == io::ErrorKind::PermissionDenied {
            return Ok(None);
        } else {
            return Err(e.into());
        }
    }

    if &buf[0..8] != MAGIC_NO {
        return Ok(None);
    }

    Ok(Some(Header {
        format_version: parse_u32(&buf, 8),
        file_no: parse_u32(&buf, 12),
    }))
}

fn encode_header(header: &Header) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_LENGTH);
    buf.extend_from_slice(MAGIC_NO);
    buf.extend_from_slice(&header.format_version.to_be_bytes());
    buf.extend_from_slice(&header.file_no.to_be_bytes());
    buf
}

/// Gets handles to all files in the directory with:
///
/// 1. the extension .data
/// 2. the magic number as their first 8 bytes
///
/// Returns them sorted by file_no
pub fn get_files(path: &str) -> Result<BTreeMap<u32, File>, StorageError> {
    if !fs::exists(path)? || !fs::metadata(path)?.is_dir() {
        return Err(StorageError::InvalidDBPath(path.into()));
    }

    let mut files: BTreeMap<u32, File> = BTreeMap::new();

    for file in fs::read_dir(path)? {
        let file = file?;

        if !file.metadata()?.is_file() {
            continue;
        }

        let file_name = file.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };

        if file.path().extension().and_then(|s| s.to_str()) != Some(EXTENSION) {
            continue;
        }

        let mut file_path = PathBuf::from(path);
        file_path.push(file_name);
        let file = open_file(file_path.to_str().unwrap(), false)?;
        let Some(header) = parse_header(&file)? else {
            continue;
        };

        files.insert(header.file_no, file);
    }

    Ok(files)
}

pub fn create_data_file(directory_path: &str, file_no: u32) -> Result<File, StorageError> {
    let file_name = format!("{}{:05}.{}", FILENAME_ROOT, file_no, EXTENSION);
    let mut file_path = PathBuf::from(directory_path);
    file_path.push(file_name);

    let file = open_file(file_path.to_str().unwrap(), true)?;

    // Set the file header.
    file.write_all_at(
        &encode_header(&Header {
            file_no,
            format_version: VERSION,
        }),
        0,
    )?;

    Ok(file)
}
