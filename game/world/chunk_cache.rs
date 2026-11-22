use crate::terrain::TerrainSample;
use crate::world::{ChunkCoord, WorldConfig};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};

const FORMAT_VERSION: u32 = 4;
const MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CACHE_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CacheKey {
    seed: u64,
    coord: ChunkCoord,
    resolution: u32,
    settings_signature: u64,
}

impl CacheKey {
    pub(crate) fn new(config: WorldConfig, coord: ChunkCoord) -> Self {
        Self {
            seed: config.seed,
            coord,
            resolution: config.chunk_size_m,
            settings_signature: settings_signature(config),
        }
    }

    fn file_name(self) -> String {
        format!(
            "{}_{}_{}_{}_{}_{}.gz",
            self.seed,
            self.coord.x,
            self.coord.z,
            self.resolution,
            FORMAT_VERSION,
            self.settings_signature
        )
    }
}

#[derive(Deserialize, Serialize)]
struct CacheHeader {
    seed: u64,
    chunk_x: i32,
    chunk_z: i32,
    resolution: u32,
    format_version: u32,
    generation_settings_signature: u64,
}

impl CacheHeader {
    fn from_key(key: CacheKey) -> Self {
        Self {
            seed: key.seed,
            chunk_x: key.coord.x,
            chunk_z: key.coord.z,
            resolution: key.resolution,
            format_version: FORMAT_VERSION,
            generation_settings_signature: key.settings_signature,
        }
    }

    fn matches(&self, key: CacheKey) -> bool {
        self.seed == key.seed
            && self.chunk_x == key.coord.x
            && self.chunk_z == key.coord.z
            && self.resolution == key.resolution
            && self.format_version == FORMAT_VERSION
            && self.generation_settings_signature == key.settings_signature
    }
}

#[derive(Deserialize, Serialize)]
struct CacheFile {
    header: CacheHeader,
    samples: Vec<TerrainSample>,
}

struct WriteRequest {
    key: CacheKey,
    samples: Vec<TerrainSample>,
}

#[derive(Clone)]
pub(crate) struct ChunkCache {
    root: PathBuf,
    sender: SyncSender<WriteRequest>,
}

pub(crate) struct CacheWriter {
    sender: Option<SyncSender<WriteRequest>>,
    worker: Option<JoinHandle<()>>,
}

impl ChunkCache {
    pub(crate) fn new(root: PathBuf) -> io::Result<(Self, CacheWriter)> {
        fs::create_dir_all(&root)?;
        let (sender, receiver) = mpsc::sync_channel::<WriteRequest>(32);
        let worker_root = root.clone();
        let worker = thread::Builder::new()
            .name("eclipse-chunk-cache".to_owned())
            .spawn(move || cache_writer(worker_root, receiver))?;

        Ok((
            Self {
                root,
                sender: sender.clone(),
            },
            CacheWriter {
                sender: Some(sender),
                worker: Some(worker),
            },
        ))
    }

    pub(crate) fn load(
        &self,
        key: CacheKey,
        expected_samples: usize,
    ) -> io::Result<Option<Vec<TerrainSample>>> {
        let path = self.root.join(key.file_name());
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if file.metadata()?.len() > MAX_CACHE_ENTRY_BYTES {
            eprintln!("Ignoring oversized terrain cache {}", path.display());
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            return Ok(None);
        }

        let decoded = (|| {
            let decoder = GzDecoder::new(file);
            let maximum_json_bytes = expected_samples
                .checked_mul(192)
                .and_then(|bytes| bytes.checked_add(512))
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "chunk cache sample count is too large",
                    )
                })?;
            let mut bytes = Vec::new();
            decoder
                .take(maximum_json_bytes as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if bytes.len() > maximum_json_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "chunk cache expands beyond its valid sample size",
                ));
            }
            let cached: CacheFile = serde_json::from_slice(&bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if !cached.header.matches(key)
                || cached.samples.len() != expected_samples
                || cached.samples.iter().any(|sample| {
                    !sample.height.is_finite()
                        || sample.normal.iter().any(|value| !value.is_finite())
                        || sample.material.iter().any(|value| !value.is_finite())
                        || sample.material[..2]
                            .iter()
                            .any(|value| !(0.0..=5.0).contains(value) || *value != value.round())
                        || !(0.0..=1.0).contains(&sample.material[2])
                })
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "chunk cache header or terrain sample data is stale",
                ));
            }
            Ok(cached.samples)
        })();

        match decoded {
            Ok(samples) => Ok(Some(samples)),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                eprintln!("Ignoring invalid terrain cache {}: {error}", path.display());
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(remove_error) if remove_error.kind() == io::ErrorKind::NotFound => {}
                    Err(remove_error) => return Err(remove_error),
                }
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn save(&self, key: CacheKey, samples: Vec<TerrainSample>) -> io::Result<()> {
        self.sender
            .send(WriteRequest { key, samples })
            .map_err(|_| io::Error::other("chunk cache writer stopped unexpectedly"))
    }
}

impl Drop for CacheWriter {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                eprintln!("chunk cache writer thread panicked");
            }
        }
    }
}

pub(crate) fn default_cache_root() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let directory = executable.parent().unwrap_or_else(|| Path::new("."));
    Ok(directory.join("gamdata").join("chunks"))
}

fn cache_writer(root: PathBuf, receiver: Receiver<WriteRequest>) {
    while let Ok(request) = receiver.recv() {
        if let Err(error) = write_cache(&root, request) {
            eprintln!("Could not write terrain chunk cache: {error}");
        }
    }
}

fn write_cache(root: &Path, request: WriteRequest) -> io::Result<()> {
    let final_path = root.join(request.key.file_name());
    if final_path.is_file() {
        return Ok(());
    }
    let cached = CacheFile {
        header: CacheHeader::from_key(request.key),
        samples: request.samples,
    };
    let bytes = serde_json::to_vec(&cached)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut temporary_path = None;
    let mut temporary_file = None;
    for _ in 0..8 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(
            ".{}.{}.{}.tmp",
            request.key.file_name(),
            std::process::id(),
            sequence
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                temporary_path = Some(path);
                temporary_file = Some(file);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let temporary_path = temporary_path
        .ok_or_else(|| io::Error::other("could not reserve a temporary cache file"))?;
    let file = temporary_file.expect("temporary path and file are created together");
    let write_result = (|| {
        let mut encoder = GzEncoder::new(file, Compression::fast());
        encoder.write_all(&bytes)?;
        let file = encoder.finish()?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        if let Err(cleanup_error) = fs::remove_file(&temporary_path) {
            if cleanup_error.kind() != io::ErrorKind::NotFound {
                return Err(io::Error::other(format!(
                    "cache write failed: {error}; temporary file cleanup failed: {cleanup_error}"
                )));
            }
        }
        return Err(error);
    }

    match fs::rename(&temporary_path, &final_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && final_path.is_file() => {
            fs::remove_file(&temporary_path)?;
        }
        Err(error) => {
            return match fs::remove_file(&temporary_path) {
                Ok(()) => Err(error),
                Err(cleanup_error) if cleanup_error.kind() == io::ErrorKind::NotFound => Err(error),
                Err(cleanup_error) => Err(io::Error::other(format!(
                    "cache rename failed: {error}; temporary file cleanup failed: {cleanup_error}"
                ))),
            };
        }
    }
    evict_old_entries(root, &final_path)
}

fn evict_old_entries(root: &Path, current: &Path) -> io::Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("gz") {
            continue;
        }
        let metadata = entry.metadata()?;
        entries.push((path, metadata.len(), metadata.modified()?));
    }
    let mut total_size = entries
        .iter()
        .fold(0_u64, |total, (_, size, _)| total.saturating_add(*size));
    entries.sort_unstable_by_key(|(_, _, modified)| *modified);

    for (path, size, _) in entries {
        if total_size <= MAX_CACHE_BYTES {
            break;
        }
        if path == current {
            continue;
        }
        match fs::remove_file(path) {
            Ok(()) => total_size = total_size.saturating_sub(size),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn settings_signature(config: WorldConfig) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for value in [
        config.chunk_size_m as u64,
        config.river_spacing_m as u64,
        config.mountain_height_m.to_bits() as u64,
    ] {
        for byte in value.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TestCacheDir(PathBuf);

    impl TestCacheDir {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "eclipse-chunk-cache-test-{}-{sequence}",
                std::process::id()
            )))
        }
    }

    impl Drop for TestCacheDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).expect("temporary chunk cache should be removable");
            }
        }
    }

    fn key() -> CacheKey {
        CacheKey::new(
            WorldConfig {
                seed: 782,
                chunk_size_m: 8,
                ..WorldConfig::default()
            },
            ChunkCoord { x: -3, z: 7 },
        )
    }

    fn samples() -> Vec<TerrainSample> {
        vec![
            TerrainSample {
                height: 1.0,
                normal: [0.0, 1.0, 0.0],
                material: [0.0, 0.0, 0.0],
            };
            9 * 9
        ]
    }

    #[test]
    fn gzip_cache_round_trips_validated_chunk_samples() {
        let directory = TestCacheDir::new();
        let original = samples();
        let (cache, writer) = ChunkCache::new(directory.0.clone()).unwrap();
        cache.save(key(), original.clone()).unwrap();
        drop(cache);
        drop(writer);

        let (cache, writer) = ChunkCache::new(directory.0.clone()).unwrap();
        assert_eq!(cache.load(key(), original.len()).unwrap(), Some(original));
        drop(cache);
        drop(writer);
    }

    #[test]
    fn corrupt_cache_entries_are_removed_for_regeneration() {
        let directory = TestCacheDir::new();
        fs::create_dir_all(&directory.0).unwrap();
        let path = directory.0.join(key().file_name());
        fs::write(&path, b"not a gzip stream").unwrap();
        let (cache, writer) = ChunkCache::new(directory.0.clone()).unwrap();

        assert!(cache.load(key(), 9 * 9).unwrap().is_none());
        assert!(!path.exists());

        drop(cache);
        drop(writer);
    }

    #[test]
    fn cache_key_tracks_terrain_settings() {
        let base = WorldConfig {
            seed: 782,
            chunk_size_m: 8,
            ..WorldConfig::default()
        };
        let coord = ChunkCoord { x: -3, z: 7 };
        assert_ne!(
            CacheKey::new(base, coord),
            CacheKey::new(
                WorldConfig {
                    mountain_height_m: base.mountain_height_m + 1.0,
                    ..base
                },
                coord
            )
        );
        assert_ne!(
            CacheKey::new(base, coord),
            CacheKey::new(
                WorldConfig {
                    seed: base.seed.wrapping_add(1),
                    ..base
                },
                coord
            )
        );
    }
}
