use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"ECLPAK01";
const HEADER_SIZE: u64 = 12;
const MAX_ASSET_PATH_BYTES: usize = 4096;

#[derive(Clone, Copy)]
struct AssetEntry {
    offset: u64,
    length: u64,
}

pub struct AssetPack {
    path: PathBuf,
    entries: HashMap<String, AssetEntry>,
}

pub struct AssetPackReader {
    file: File,
    offset: u64,
    length: u64,
    position: u64,
}

#[derive(Default)]
pub struct AssetPackBuilder {
    entries: Vec<(String, PathBuf, u64)>,
    names: HashMap<String, ()>,
}

impl AssetPack {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let file_length = file.metadata()?.len();

        let mut magic = [0; MAGIC.len()];
        file.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(
                io::Error::new(io::ErrorKind::InvalidData, "invalid asset pack header").into(),
            );
        }

        let entry_count = read_u32(&mut file)?;
        if u64::from(entry_count) > file_length.saturating_sub(HEADER_SIZE) / 19 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid asset pack entry count",
            )
            .into());
        }
        let mut entries = HashMap::with_capacity(entry_count as usize);
        let mut ranges = Vec::with_capacity(entry_count as usize);
        for _ in 0..entry_count {
            let path_length = read_u16(&mut file)? as usize;
            if path_length == 0 || path_length > MAX_ASSET_PATH_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid asset path length",
                )
                .into());
            }
            let mut path_bytes = vec![0; path_length];
            file.read_exact(&mut path_bytes)?;
            let name = String::from_utf8(path_bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let name = normalize_asset_path(&name)?;
            let offset = read_u64(&mut file)?;
            let length = read_u64(&mut file)?;
            if entries
                .insert(name, AssetEntry { offset, length })
                .is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate asset pack path",
                )
                .into());
            }
            ranges.push((offset, length));
        }

        let data_start = file.stream_position()?;
        if ranges.iter().any(|(offset, length)| {
            *offset < data_start
                || offset
                    .checked_add(*length)
                    .map(|end| end > file_length)
                    .unwrap_or(true)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "asset pack entry is out of bounds",
            )
            .into());
        }

        Ok(Self { path, entries })
    }

    pub fn contains(&self, asset_path: &str) -> bool {
        normalize_asset_path(asset_path)
            .map(|path| self.entries.contains_key(&path))
            .unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn open_asset(
        &self,
        asset_path: &str,
    ) -> Result<AssetPackReader, Box<dyn std::error::Error>> {
        let asset_path = normalize_asset_path(asset_path)?;
        let entry = self.entries.get(&asset_path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("asset '{asset_path}' is not in {}", self.path.display()),
            )
        })?;
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(entry.offset))?;
        Ok(AssetPackReader {
            file,
            offset: entry.offset,
            length: entry.length,
            position: 0,
        })
    }
}

impl AssetPackReader {
    pub fn len(&self) -> u64 {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}

impl Read for AssetPackReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.length - self.position;
        let read_length = if remaining < buffer.len() as u64 {
            remaining as usize
        } else {
            buffer.len()
        };
        if read_length == 0 {
            return Ok(0);
        }
        let read = self.file.read(&mut buffer[..read_length])?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for AssetPackReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let target = match position {
            SeekFrom::Start(position) => position as i128,
            SeekFrom::End(offset) => self.length as i128 + offset as i128,
            SeekFrom::Current(offset) => self.position as i128 + offset as i128,
        };
        if target < 0 || target > self.length as i128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "asset seek is outside the entry",
            ));
        }
        self.file
            .seek(SeekFrom::Start(self.offset + target as u64))?;
        self.position = target as u64;
        Ok(self.position)
    }
}

impl AssetPackBuilder {
    pub fn add_file(
        &mut self,
        asset_path: &str,
        source_path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let asset_path = normalize_asset_path(asset_path)?;
        if asset_path.len() > MAX_ASSET_PATH_BYTES || asset_path.len() > u16::MAX as usize {
            return Err(
                io::Error::new(io::ErrorKind::InvalidInput, "asset path is too long").into(),
            );
        }
        if self.names.insert(asset_path.clone(), ()).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate asset path '{asset_path}'"),
            )
            .into());
        }
        let source_path = source_path.as_ref().to_path_buf();
        let length = fs::metadata(&source_path)?.len();
        self.entries.push((asset_path, source_path, length));
        Ok(())
    }

    pub fn write_to(self, output_path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
        let output_path = output_path.as_ref();
        let entry_count = u32::try_from(self.entries.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many assets"))?;
        let table_length = self.entries.iter().try_fold(0_u64, |total, (name, _, _)| {
            total
                .checked_add(2 + name.len() as u64 + 16)
                .ok_or_else(|| io::Error::other("asset pack index size overflow"))
        })?;
        let mut next_offset = HEADER_SIZE
            .checked_add(table_length)
            .ok_or_else(|| io::Error::other("asset pack size overflow"))?;
        let mut offsets = Vec::with_capacity(self.entries.len());
        for (_, _, length) in &self.entries {
            offsets.push(next_offset);
            next_offset = next_offset
                .checked_add(*length)
                .ok_or_else(|| io::Error::other("asset pack size overflow"))?;
        }

        if let Some(parent) = output_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(output_path)?;
        output.write_all(MAGIC)?;
        output.write_all(&entry_count.to_le_bytes())?;
        for ((name, _, length), offset) in self.entries.iter().zip(offsets) {
            output.write_all(&(name.len() as u16).to_le_bytes())?;
            output.write_all(name.as_bytes())?;
            output.write_all(&offset.to_le_bytes())?;
            output.write_all(&length.to_le_bytes())?;
        }
        for (name, source_path, expected_length) in self.entries {
            let mut source = File::open(&source_path)?;
            let copied_length = io::copy(&mut source, &mut output)?;
            if copied_length != expected_length {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "asset '{name}' changed size while packing: expected {expected_length}, copied {copied_length}"
                    ),
                )
                .into());
            }
        }
        output.flush()?;
        Ok(())
    }
}

fn normalize_asset_path(path: &str) -> Result<String, io::Error> {
    let path = path.replace('\\', "/");
    if path.is_empty()
        || path.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "asset path must be a normalized relative path",
        ));
    }
    Ok(path)
}

fn read_u16(reader: &mut impl Read) -> io::Result<u16> {
    let mut bytes = [0; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::{AssetPack, AssetPackBuilder};
    use std::fs;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn pack_entries_can_be_read_and_seeked_without_loading_the_pack() {
        let temp_dir = std::env::temp_dir().join(format!(
            "eclipse-asset-pack-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        let source = temp_dir.join("source.bin");
        let pack_path = temp_dir.join("assets.pak");
        fs::File::create(&source)
            .unwrap()
            .write_all(b"on-demand asset")
            .unwrap();

        let mut builder = AssetPackBuilder::default();
        builder.add_file("sound/sfx/test.bin", &source).unwrap();
        builder.write_to(&pack_path).unwrap();

        let pack = AssetPack::open(&pack_path).unwrap();
        assert!(pack.contains("sound/sfx/test.bin"));
        let mut asset = pack.open_asset("sound/sfx/test.bin").unwrap();
        asset.seek(SeekFrom::Start(10)).unwrap();
        let mut tail = Vec::new();
        asset.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, b"asset");

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn pack_paths_must_be_relative_and_unique() {
        let source = std::env::current_exe().unwrap();
        let mut builder = AssetPackBuilder::default();
        assert!(builder.add_file("../outside", &source).is_err());
        builder.add_file("icons/test.png", &source).unwrap();
        assert!(builder.add_file("icons/test.png", source).is_err());
    }
}
