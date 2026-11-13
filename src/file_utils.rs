use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;

use crate::storage::StorageError;

// Pahther DB magic number: 0PNTH\9e\fa\b4
const MAGIC_NO: &[u8] = &[0x00, 0x70, 0x6E, 0x74, 0x68, 0x9e, 0xfa, 0xb4];
pub(crate) const EXTENSION: &str = "data";
pub(crate) const FILENAME_ROOT: &str = "pantherdb_data_";
// Hardcode version to 0 for now.
// I can allow configuring it later if the storage format evolves.
const VERSION: u32 = 0;
pub(crate) const HEADER_LENGTH: usize = 16;

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

pub fn file_name(file_no: u32) -> String {
    format!("{}{:05}.{}", FILENAME_ROOT, file_no, EXTENSION)
}

pub fn file_path(file_no: u32, directory_path: &str) -> String {
    let mut file_path = PathBuf::from(directory_path);
    file_path.push(file_name(file_no));
    file_path.to_str().unwrap().to_string()
}

pub fn create_data_file(directory_path: &str, file_no: u32) -> Result<File, StorageError> {
    let file = open_file(&file_path(file_no, directory_path), true)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("keyval-file-utils-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn write_file(path: &Path, contents: &[u8]) {
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn parses_big_endian_integers_at_offsets() {
        let bytes = [0xff, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];

        assert_eq!(parse_u32(&bytes, 1), 0x0102_0304);
        assert_eq!(parse_u64(&bytes, 1), 0x0102_0304_0506_0708);
    }

    #[test]
    fn formats_file_names_and_paths() {
        assert_eq!(file_name(0), "pantherdb_data_00000.data");
        assert_eq!(file_name(42), "pantherdb_data_00042.data");
        assert_eq!(file_name(u32::MAX), "pantherdb_data_4294967295.data");
        assert_eq!(
            file_path(7, "/database"),
            "/database/pantherdb_data_00007.data"
        );
    }

    #[test]
    fn creates_a_header_for_the_requested_file_number() {
        let temp_dir = TempDir::new();
        let file = create_data_file(temp_dir.path().to_str().unwrap(), 37).unwrap();

        let header = parse_header(&file).unwrap().unwrap();
        assert_eq!(header.format_version, VERSION);
        assert_eq!(header.file_no, 37);
    }

    #[test]
    fn get_files_returns_only_regular_data_files_with_the_magic_header() {
        let temp_dir = TempDir::new();
        let path = temp_dir.path();

        create_data_file(path.to_str().unwrap(), 10).unwrap();
        create_data_file(path.to_str().unwrap(), 2).unwrap();
        write_file(
            &path.join("wrong-extension.txt"),
            &encode_header(&Header {
                format_version: VERSION,
                file_no: 20,
            }),
        );
        let mut bad_magic = encode_header(&Header {
            format_version: VERSION,
            file_no: 30,
        });
        bad_magic[0] ^= 0xff;
        write_file(&path.join("bad-magic.data"), &bad_magic);
        write_file(&path.join("truncated.data"), &MAGIC_NO[..7]);
        fs::create_dir(path.join("directory.data")).unwrap();

        let files = get_files(path.to_str().unwrap()).unwrap();

        assert_eq!(files.keys().copied().collect::<Vec<_>>(), [2, 10]);
    }

    #[test]
    fn get_files_rejects_missing_paths_and_non_directories() {
        let temp_dir = TempDir::new();
        let file_path = temp_dir.path().join("not-a-directory");
        write_file(&file_path, b"");

        assert!(matches!(
            get_files(temp_dir.path().join("missing").to_str().unwrap()),
            Err(StorageError::InvalidDBPath(_))
        ));
        assert!(matches!(
            get_files(file_path.to_str().unwrap()),
            Err(StorageError::InvalidDBPath(_))
        ));
    }
}
