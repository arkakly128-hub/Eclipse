use crate::chunk_cache::{CacheKey, CacheWriter, ChunkCache};
use crate::terrain;
use crate::tree::{TreeChunk, TreeForest};
use eclipse_engine::collision::{Ray, RaycastHit, TriangleMeshCollider};
use eclipse_engine::mesh::{ChunkedMesh, MeshChunk};
use glam::Vec3;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

const MAX_SPAWN_GROUND_SLOPE: f32 = 0.7;
const UNLOAD_PADDING_CHUNKS: i32 = 2;
const MAX_CHUNKS_PER_FRAME: usize = 4;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChunkCoord {
    pub x: i32,
    pub z: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldConfig {
    pub chunk_size_m: u32,
    pub streaming_radius_chunks: i32,
    pub seed: u64,
    pub tree_spacing_m: u32,
    pub river_spacing_m: u32,
    pub mountain_height_m: f32,
    pub gameplay: GameplayConfig,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GameplayConfig {
    pub day_length_seconds: f64,
    pub start_hour: f32,
    pub max_health_units: f32,
    pub max_stamina_units: f32,
    pub max_food_units: f32,
    pub food_depletion_seconds: f32,
    pub sprint_stamina_per_second: f32,
    pub jump_stamina_cost_units: f32,
    pub stamina_regen_per_second: f32,
    pub food_per_stamina_unit: f32,
    pub health_regen_per_second: f32,
    pub health_regen_food_per_second: f32,
    pub starvation_damage_per_second: f32,
    pub fall_safe_distance_m: f32,
    pub fall_damage_per_meter: f32,
}

impl Default for GameplayConfig {
    fn default() -> Self {
        Self {
            day_length_seconds: 900.0,
            start_hour: 6.0,
            max_health_units: 100.0,
            max_stamina_units: 100.0,
            max_food_units: 100.0,
            food_depletion_seconds: 2_857.142_8,
            sprint_stamina_per_second: 18.0,
            jump_stamina_cost_units: 18.0,
            stamina_regen_per_second: 18.0,
            food_per_stamina_unit: 0.01,
            health_regen_per_second: 1.0,
            health_regen_food_per_second: 0.5,
            starvation_damage_per_second: 5.0,
            fall_safe_distance_m: 3.0,
            fall_damage_per_meter: 12.0,
        }
    }
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            chunk_size_m: 64,
            streaming_radius_chunks: 4,
            seed: 454_138,
            tree_spacing_m: 14,
            river_spacing_m: 320,
            mountain_height_m: 220.0,
            gameplay: GameplayConfig::default(),
        }
    }
}

#[derive(Debug)]
struct WorldChunk {
    terrain: Arc<ChunkedMesh>,
    river: Arc<ChunkedMesh>,
    ocean: Arc<ChunkedMesh>,
    terrain_collision: TriangleMeshCollider,
    trees: Arc<TreeChunk>,
}

#[derive(Default)]
struct ReusableChunk {
    terrain: ChunkedMesh,
    river: ChunkedMesh,
    ocean: ChunkedMesh,
}

impl WorldChunk {
    fn into_reusable(self) -> Option<ReusableChunk> {
        let Self {
            terrain,
            river,
            ocean,
            terrain_collision,
            trees,
        } = self;
        drop(terrain_collision);
        drop(trees);
        Some(ReusableChunk {
            terrain: Arc::try_unwrap(terrain).ok()?,
            river: Arc::try_unwrap(river).ok()?,
            ocean: Arc::try_unwrap(ocean).ok()?,
        })
    }
}

#[derive(Default)]
struct ChunkPool {
    residents: HashMap<ChunkCoord, Arc<WorldChunk>>,
    reusable: Vec<ReusableChunk>,
}

impl ChunkPool {
    fn retain_near(&mut self, center: ChunkCoord, radius: i32) {
        let expired = self
            .residents
            .keys()
            .filter(|coord| chunk_distance(center, **coord) > i64::from(radius))
            .copied()
            .collect::<Vec<_>>();
        for coord in expired {
            if let Some(chunk) = self.residents.remove(&coord) {
                if let Ok(chunk) = Arc::try_unwrap(chunk) {
                    if let Some(reusable) = chunk.into_reusable() {
                        self.reusable.push(reusable);
                    }
                }
            }
        }
        self.reusable.truncate(32);
    }

    fn take_reusable(&mut self, count: usize) -> Vec<ReusableChunk> {
        (0..count.min(self.reusable.len()))
            .filter_map(|_| self.reusable.pop())
            .collect()
    }
}

impl Deref for ChunkPool {
    type Target = HashMap<ChunkCoord, Arc<WorldChunk>>;

    fn deref(&self) -> &Self::Target {
        &self.residents
    }
}

impl DerefMut for ChunkPool {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.residents
    }
}

pub struct World {
    pub config: WorldConfig,
    pub terrain: Arc<ChunkedMesh>,
    pub water: Arc<ChunkedMesh>,
    pub tree_mesh: ChunkedMesh,
    pub trees: TreeForest,
    cache: ChunkCache,
    chunks: ChunkPool,
    stream_center: Option<ChunkCoord>,
    streaming_result: Option<Receiver<Result<(ChunkCoord, Arc<WorldChunk>), io::Error>>>,
    render_center: ChunkCoord,
    _cache_writer: CacheWriter,
}

pub fn load() -> Result<World, Box<dyn Error>> {
    let path = world_config_path()?;
    let source = fs::read_to_string(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot read world config {}: {error}", path.display()),
        )
    })?;
    let config = load_config(&source).map_err(|error| {
        io::Error::other(format!("invalid world config {}: {error}", path.display()))
    })?;
    World::new(config)
}

impl World {
    fn new(config: WorldConfig) -> Result<Self, Box<dyn Error>> {
        let (cache, cache_writer) = ChunkCache::new(crate::chunk_cache::default_cache_root()?)?;
        Self::new_with_cache(config, cache, cache_writer)
    }

    fn new_with_cache(
        config: WorldConfig,
        cache: ChunkCache,
        cache_writer: CacheWriter,
    ) -> Result<Self, Box<dyn Error>> {
        validate_config(&config)?;
        let center = ChunkCoord { x: 0, z: 0 };
        let spawn_neighborhood = [
            ChunkCoord { x: -1, z: -1 },
            ChunkCoord { x: 0, z: -1 },
            ChunkCoord { x: -1, z: 0 },
            center,
        ];
        let initial_chunks =
            generate_chunks_parallel(config, &spawn_neighborhood, cache.clone(), Vec::new())?;
        let chunks = ChunkPool {
            residents: initial_chunks.into_iter().collect(),
            reusable: Vec::new(),
        };
        let desired = spawn_neighborhood;
        let terrain_mesh = combine_chunks(&chunks, &desired, config.chunk_size_m);
        let water_mesh = combine_water_chunks(&chunks, &desired);
        let trees = tree_forest_from_chunks(config, &chunks, &desired);
        Ok(Self {
            config,
            terrain: Arc::new(terrain_mesh),
            water: Arc::new(water_mesh),
            tree_mesh: TreeForest::build_mesh()?,
            trees,
            cache,
            chunks,
            stream_center: None,
            streaming_result: None,
            render_center: center,
            _cache_writer: cache_writer,
        })
    }

    pub fn update_streaming(&mut self, position: Vec3) -> Result<bool, Box<dyn Error>> {
        if !position.x.is_finite() || !position.z.is_finite() {
            return Err("cannot stream world around a non-finite position".into());
        }

        let center = chunk_coord_at(position.x, position.z, self.config.chunk_size_m)
            .ok_or("world position is outside the supported chunk coordinate range")?;
        let center_changed = self.render_center != center;
        let desired = desired_chunks(center, self.config.streaming_radius_chunks);
        let mut visible_chunk_loaded = false;
        let mut disconnected = false;
        if let Some(receiver) = &self.streaming_result {
            for _ in 0..MAX_CHUNKS_PER_FRAME {
                match receiver.try_recv() {
                    Ok(Ok((coord, chunk))) => {
                        visible_chunk_loaded |= desired.contains(&coord);
                        self.chunks.insert(coord, chunk);
                    }
                    Ok(Err(error)) => return Err(error.into()),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if disconnected {
            self.streaming_result = None;
        }

        self.chunks.retain_near(
            center,
            self.config.streaming_radius_chunks + UNLOAD_PADDING_CHUNKS,
        );
        self.render_center = center;
        let mut updated = false;
        if center_changed || visible_chunk_loaded {
            updated = self.rebuild_visible_region(center);
        }

        if self.streaming_result.is_none() && self.stream_center != Some(center) {
            self.start_streaming_worker(center)?;
        } else if self.streaming_result.is_none()
            && desired.iter().any(|coord| !self.chunks.contains_key(coord))
        {
            self.start_streaming_worker(center)?;
        }
        Ok(updated)
    }

    fn start_streaming_worker(&mut self, center: ChunkCoord) -> Result<(), Box<dyn Error>> {
        let config = self.config;
        let cache = self.cache.clone();
        let resident = self.chunks.keys().copied().collect::<Vec<_>>();
        let coordinates = desired_chunks(center, config.streaming_radius_chunks)
            .into_iter()
            .filter(|coord| !resident.contains(coord))
            .collect::<Vec<_>>();
        let reusable = self.chunks.take_reusable(coordinates.len());
        self.stream_center = Some(center);
        if coordinates.is_empty() {
            return Ok(());
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("eclipse-world-stream".to_owned())
            .spawn(move || {
                let mut reusable = reusable;
                for ring in 0..=config.streaming_radius_chunks {
                    let ring_coordinates = coordinates
                        .iter()
                        .copied()
                        .filter(|coord| chunk_distance(center, *coord) == i64::from(ring))
                        .collect::<Vec<_>>();
                    if ring_coordinates.is_empty() {
                        continue;
                    }
                    let reusable_count = ring_coordinates.len().min(reusable.len());
                    let ring_reusable = reusable.drain(..reusable_count).collect();
                    match generate_chunks_parallel(
                        config,
                        &ring_coordinates,
                        cache.clone(),
                        ring_reusable,
                    ) {
                        Ok(chunks) => {
                            for (coord, chunk) in chunks {
                                if sender.send(Ok((coord, chunk))).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(error) => {
                            let _ = sender.send(Err(error));
                            return;
                        }
                    }
                }
            })?;
        self.streaming_result = Some(receiver);
        Ok(())
    }

    fn rebuild_visible_region(&mut self, center: ChunkCoord) -> bool {
        let desired = desired_chunks(center, self.config.streaming_radius_chunks);
        if !desired.iter().any(|coord| self.chunks.contains_key(coord)) {
            return false;
        }
        let terrain = combine_chunks(&self.chunks, &desired, self.config.chunk_size_m);
        if terrain.indices.is_empty() {
            return false;
        }
        self.water = Arc::new(combine_water_chunks(&self.chunks, &desired));
        self.terrain = Arc::new(terrain);
        self.trees = tree_forest_from_chunks(self.config, &self.chunks, &desired);
        true
    }

    pub fn raycast_ground(&self, ray: Ray, max_distance: f32) -> Option<RaycastHit> {
        let coord = chunk_coord_at(ray.origin[0], ray.origin[2], self.config.chunk_size_m)?;
        self.chunks
            .get(&coord)
            .and_then(|chunk| chunk.terrain_collision.raycast(ray, max_distance))
    }

    pub fn spawn_position(&self) -> Vec3 {
        const SPAWN_RADIUS_M: i32 = 3;
        const SPAWN_X_SALT: u64 = 0x6a09_e667;
        const SPAWN_Z_SALT: u64 = 0xbb67_ae85;
        let preferred = (
            terrain::hash_unit(0, 0, self.config.seed, SPAWN_X_SALT) * (SPAWN_RADIUS_M * 2) as f32
                - SPAWN_RADIUS_M as f32,
            terrain::hash_unit(0, 0, self.config.seed, SPAWN_Z_SALT) * (SPAWN_RADIUS_M * 2) as f32
                - SPAWN_RADIUS_M as f32,
        );
        let mut candidates = vec![preferred];
        for z in -SPAWN_RADIUS_M..=SPAWN_RADIUS_M {
            for x in -SPAWN_RADIUS_M..=SPAWN_RADIUS_M {
                candidates.push((x as f32, z as f32));
            }
        }
        candidates.sort_by(|left, right| {
            let left_distance = (left.0 - preferred.0).powi(2) + (left.1 - preferred.1).powi(2);
            let right_distance = (right.0 - preferred.0).powi(2) + (right.1 - preferred.1).powi(2);
            left_distance.total_cmp(&right_distance)
        });

        let mut fallback = (preferred, f32::INFINITY);
        for (x, z) in candidates {
            let slope = terrain::ground_slope(self.config, x, z);
            let in_river = crate::river::surface_at(self.config, x, z).is_some();
            let in_ravine = crate::ravine::is_ravine(self.config, x, z, 2.0);
            let in_ocean = crate::ocean::is_ocean(self.config, x, z);
            let water_penalty = if in_ocean {
                1_000.0
            } else if in_river || in_ravine {
                100.0
            } else {
                0.0
            };
            let penalty = slope + water_penalty;
            if penalty < fallback.1 {
                fallback = ((x, z), penalty);
            }
            if slope <= MAX_SPAWN_GROUND_SLOPE && !in_river && !in_ravine && !in_ocean {
                return Vec3::new(x, terrain::height_at(self.config, x, z), z);
            }
        }

        let (x, z) = fallback.0;
        Vec3::new(x, terrain::height_at(self.config, x, z), z)
    }

    #[cfg(test)]
    pub fn is_safe_spawn_ground(&self, x: f32, z: f32) -> bool {
        terrain::ground_slope(self.config, x, z) <= MAX_SPAWN_GROUND_SLOPE
    }

    pub fn water_surface_at_time(&self, x: f32, z: f32, time: f32) -> Option<f32> {
        if crate::ocean::is_ocean(self.config, x, z) {
            Some(
                crate::ocean::SEA_LEVEL
                    + crate::water_waves::height(x, z, time, crate::water_waves::OCEAN_STYLE),
            )
        } else {
            crate::river::surface_at(self.config, x, z).map(|surface| {
                surface + crate::water_waves::height(x, z, time, crate::water_waves::RIVER_STYLE)
            })
        }
    }
}

fn world_config_path() -> Result<PathBuf, Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    let executable_dir = executable.parent().unwrap_or_else(|| Path::new("."));
    let packaged_parent = executable_dir.parent().unwrap_or(executable_dir);
    let candidates = [
        executable_dir.join("world.cfg"),
        packaged_parent.join("world.cfg"),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("world")
            .join("world.cfg"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "world.cfg not found beside the executable, in its parent, or in {}",
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("world")
                        .display()
                ),
            )
            .into()
        })
}

fn generate_chunks_parallel(
    config: WorldConfig,
    coordinates: &[ChunkCoord],
    cache: ChunkCache,
    reusable: Vec<ReusableChunk>,
) -> Result<Vec<(ChunkCoord, Arc<WorldChunk>)>, io::Error> {
    if coordinates.is_empty() {
        return Ok(Vec::new());
    }

    let available_workers = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(2)
        .max(2)
        .min(8);
    let worker_count = available_workers.min(coordinates.len());
    let reusable = Mutex::new(reusable);
    std::thread::scope(|scope| {
        let handles = (0..worker_count)
            .map(|worker_index| {
                let cache = cache.clone();
                let reusable = &reusable;
                scope.spawn(move || {
                    coordinates
                        .iter()
                        .skip(worker_index)
                        .step_by(worker_count)
                        .map(|coord| {
                            let reusable = reusable
                                .lock()
                                .map_err(|_| io::Error::other("chunk pool lock was poisoned"))?
                                .pop();
                            generate_world_chunk(config, *coord, &cache, reusable)
                                .map(|chunk| (*coord, chunk))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
            })
            .collect::<Vec<_>>();

        let mut generated = Vec::with_capacity(coordinates.len());
        for handle in handles {
            match handle.join() {
                Ok(Ok(chunks)) => generated.extend(chunks),
                Ok(Err(error)) => return Err(error),
                Err(_) => return Err(io::Error::other("terrain chunk worker panicked")),
            }
        }
        Ok(generated)
    })
}

fn generate_world_chunk(
    config: WorldConfig,
    coord: ChunkCoord,
    cache: &ChunkCache,
    reusable: Option<ReusableChunk>,
) -> Result<Arc<WorldChunk>, io::Error> {
    let key = CacheKey::new(config, coord);
    let side = config.chunk_size_m as usize + 1;
    let expected_samples = side
        .checked_mul(side)
        .ok_or_else(|| io::Error::other("terrain sample count exceeds addressable memory"))?;
    let samples = match cache.load(key, expected_samples)? {
        Some(samples) => samples,
        None => {
            let samples = terrain::generate_samples(config, coord);
            cache.save(key, samples.clone())?;
            samples
        }
    };
    let mut reusable = reusable.unwrap_or_default();
    terrain::assemble_chunk_into(config, coord, &samples, &mut reusable.terrain);
    crate::river::generate_chunk_into(config, coord, &mut reusable.river);
    crate::ocean::generate_chunk_into(config, coord, &mut reusable.ocean);
    let terrain = Arc::new(reusable.terrain);
    let river = Arc::new(reusable.river);
    let ocean = Arc::new(reusable.ocean);
    let trees = Arc::new(TreeForest::generate_chunk(
        config,
        coord,
        terrain::height_at,
    ));
    let terrain_collision =
        TriangleMeshCollider::new(Arc::clone(&terrain)).map_err(io::Error::other)?;
    Ok(Arc::new(WorldChunk {
        terrain,
        river,
        ocean,
        terrain_collision,
        trees,
    }))
}

fn desired_chunks(center: ChunkCoord, radius: i32) -> Vec<ChunkCoord> {
    let mut desired = Vec::with_capacity(((radius * 2 + 1).pow(2)) as usize);
    for z in center.z.saturating_sub(radius)..=center.z.saturating_add(radius) {
        for x in center.x.saturating_sub(radius)..=center.x.saturating_add(radius) {
            desired.push(ChunkCoord { x, z });
        }
    }
    desired.sort_unstable_by_key(|coord| {
        let dx = i64::from(coord.x) - i64::from(center.x);
        let dz = i64::from(coord.z) - i64::from(center.z);
        (
            chunk_distance(center, *coord),
            dx * dx + dz * dz,
            coord.x,
            coord.z,
        )
    });
    desired
}

fn chunk_distance(a: ChunkCoord, b: ChunkCoord) -> i64 {
    (i64::from(a.x) - i64::from(b.x))
        .abs()
        .max((i64::from(a.z) - i64::from(b.z)).abs())
}

fn chunk_coord_at(x: f32, z: f32, chunk_size_m: u32) -> Option<ChunkCoord> {
    fn component(position: f32, chunk_size_m: u32) -> Option<i32> {
        if !position.is_finite() || chunk_size_m == 0 {
            return None;
        }
        let coordinate = (f64::from(position) / f64::from(chunk_size_m)).floor();
        (f64::from(i32::MIN)..=f64::from(i32::MAX))
            .contains(&coordinate)
            .then_some(coordinate as i32)
    }

    Some(ChunkCoord {
        x: component(x, chunk_size_m)?,
        z: component(z, chunk_size_m)?,
    })
}

fn tree_forest_from_chunks(
    config: WorldConfig,
    chunks: &HashMap<ChunkCoord, Arc<WorldChunk>>,
    desired: &[ChunkCoord],
) -> TreeForest {
    let trees = desired
        .iter()
        .filter_map(|coord| {
            chunks
                .get(coord)
                .map(|chunk| (*coord, Arc::clone(&chunk.trees)))
        })
        .collect::<Vec<_>>();
    TreeForest::from_chunks(config, &trees)
}

fn combine_chunks(
    chunks: &HashMap<ChunkCoord, Arc<WorldChunk>>,
    ordered: &[ChunkCoord],
    chunk_size_m: u32,
) -> ChunkedMesh {
    let cells_per_chunk = chunk_size_m as usize;
    let vertices_per_chunk = ordered.len() * (cells_per_chunk + 1).pow(2);
    let house_vertex_capacity = ordered.len() * 64;
    let mut combined = ChunkedMesh::with_capacity(
        vertices_per_chunk + house_vertex_capacity,
        cells_per_chunk * cells_per_chunk * ordered.len() * 6 + ordered.len() * 384,
        ordered.len(),
    );
    for coord in ordered {
        let Some(chunk) = chunks.get(coord) else {
            continue;
        };
        let vertex_offset = combined.vertices.len() as u32;
        let index_offset = combined.indices.len();
        combined.vertices.extend_from_slice(&chunk.terrain.vertices);
        combined.indices.extend(
            chunk
                .terrain
                .indices
                .iter()
                .map(|index| index + vertex_offset),
        );
        for range in &chunk.terrain.chunks {
            combined.chunks.push(MeshChunk {
                origin: range.origin,
                indices: (range.indices.start + index_offset)..(range.indices.end + index_offset),
            });
        }
    }
    combined
}

fn combine_water_chunks(
    chunks: &HashMap<ChunkCoord, Arc<WorldChunk>>,
    ordered: &[ChunkCoord],
) -> ChunkedMesh {
    let (vertex_count, index_count, chunk_count) = ordered
        .iter()
        .filter_map(|coord| chunks.get(coord))
        .fold((0, 0, 0), |(vertices, indices, ranges), chunk| {
            (
                vertices + chunk.river.vertices.len() + chunk.ocean.vertices.len(),
                indices + chunk.river.indices.len() + chunk.ocean.indices.len(),
                ranges + chunk.river.chunks.len() + chunk.ocean.chunks.len(),
            )
        });
    let mut combined = ChunkedMesh::with_capacity(vertex_count, index_count, chunk_count);
    for coord in ordered {
        let Some(chunk) = chunks.get(coord) else {
            continue;
        };
        for mesh in [&chunk.river, &chunk.ocean] {
            if mesh.indices.is_empty() {
                continue;
            }
            let vertex_offset = combined.vertices.len() as u32;
            let index_offset = combined.indices.len();
            combined.vertices.extend_from_slice(&mesh.vertices);
            combined
                .indices
                .extend(mesh.indices.iter().map(|index| index + vertex_offset));
            for range in &mesh.chunks {
                combined.chunks.push(MeshChunk {
                    origin: range.origin,
                    indices: (range.indices.start + index_offset)
                        ..(range.indices.end + index_offset),
                });
            }
        }
    }
    combined
}

fn load_config(config: &str) -> Result<WorldConfig, Box<dyn Error>> {
    let mut chunk_size_m = None;
    let mut streaming_radius_chunks = None;
    let mut seed = None;
    let mut tree_spacing_m = None;
    let mut river_spacing_m = None;
    let mut mountain_height_m = None;
    let mut day_length_seconds = None;
    let mut start_hour = None;
    let mut max_health_units = None;
    let mut max_stamina_units = None;
    let mut max_food_units = None;
    let mut food_depletion_seconds = None;
    let mut sprint_stamina_per_second = None;
    let mut jump_stamina_cost_units = None;
    let mut stamina_regen_per_second = None;
    let mut food_per_stamina_unit = None;
    let mut health_regen_per_second = None;
    let mut health_regen_food_per_second = None;
    let mut starvation_damage_per_second = None;
    let mut fall_safe_distance_m = None;
    let mut fall_damage_per_meter = None;

    for (line_number, line) in config.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("invalid world config at line {}", line_number + 1).into());
        };
        match key.trim() {
            "chunk_size_m" => chunk_size_m = Some(value.trim().parse()?),
            "streaming_radius_chunks" => streaming_radius_chunks = Some(value.trim().parse()?),
            "seed" => seed = Some(value.trim().parse()?),
            "tree_spacing_m" => tree_spacing_m = Some(value.trim().parse()?),
            "river_spacing_m" => river_spacing_m = Some(value.trim().parse()?),
            "mountain_height_m" => mountain_height_m = Some(value.trim().parse()?),
            "day_length_seconds" => day_length_seconds = Some(value.trim().parse()?),
            "start_hour" => start_hour = Some(value.trim().parse()?),
            "max_health_units" => max_health_units = Some(value.trim().parse()?),
            "max_stamina_units" => max_stamina_units = Some(value.trim().parse()?),
            "max_food_units" => max_food_units = Some(value.trim().parse()?),
            "food_depletion_seconds" => food_depletion_seconds = Some(value.trim().parse()?),
            "sprint_stamina_per_second" => sprint_stamina_per_second = Some(value.trim().parse()?),
            "jump_stamina_cost_units" => jump_stamina_cost_units = Some(value.trim().parse()?),
            "stamina_regen_per_second" => stamina_regen_per_second = Some(value.trim().parse()?),
            "food_per_stamina_unit" => food_per_stamina_unit = Some(value.trim().parse()?),
            "health_regen_per_second" => health_regen_per_second = Some(value.trim().parse()?),
            "health_regen_food_per_second" => {
                health_regen_food_per_second = Some(value.trim().parse()?)
            }
            "starvation_damage_per_second" => {
                starvation_damage_per_second = Some(value.trim().parse()?)
            }
            "fall_safe_distance_m" => fall_safe_distance_m = Some(value.trim().parse()?),
            "fall_damage_per_meter" => fall_damage_per_meter = Some(value.trim().parse()?),
            _ => return Err(format!("unknown world config key: {}", key.trim()).into()),
        }
    }

    let config = WorldConfig {
        chunk_size_m: chunk_size_m.ok_or("missing chunk_size_m")?,
        streaming_radius_chunks: streaming_radius_chunks
            .ok_or("missing streaming_radius_chunks")?,
        seed: seed.ok_or("missing seed")?,
        tree_spacing_m: tree_spacing_m.ok_or("missing tree_spacing_m")?,
        river_spacing_m: river_spacing_m.ok_or("missing river_spacing_m")?,
        mountain_height_m: mountain_height_m.ok_or("missing mountain_height_m")?,
        gameplay: GameplayConfig {
            day_length_seconds: day_length_seconds.ok_or("missing day_length_seconds")?,
            start_hour: start_hour.ok_or("missing start_hour")?,
            max_health_units: max_health_units.ok_or("missing max_health_units")?,
            max_stamina_units: max_stamina_units.ok_or("missing max_stamina_units")?,
            max_food_units: max_food_units.ok_or("missing max_food_units")?,
            food_depletion_seconds: food_depletion_seconds
                .ok_or("missing food_depletion_seconds")?,
            sprint_stamina_per_second: sprint_stamina_per_second
                .ok_or("missing sprint_stamina_per_second")?,
            jump_stamina_cost_units: jump_stamina_cost_units
                .ok_or("missing jump_stamina_cost_units")?,
            stamina_regen_per_second: stamina_regen_per_second
                .ok_or("missing stamina_regen_per_second")?,
            food_per_stamina_unit: food_per_stamina_unit.ok_or("missing food_per_stamina_unit")?,
            health_regen_per_second: health_regen_per_second
                .ok_or("missing health_regen_per_second")?,
            health_regen_food_per_second: health_regen_food_per_second
                .ok_or("missing health_regen_food_per_second")?,
            starvation_damage_per_second: starvation_damage_per_second
                .ok_or("missing starvation_damage_per_second")?,
            fall_safe_distance_m: fall_safe_distance_m.ok_or("missing fall_safe_distance_m")?,
            fall_damage_per_meter: fall_damage_per_meter.ok_or("missing fall_damage_per_meter")?,
        },
    };
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &WorldConfig) -> Result<(), Box<dyn Error>> {
    let gameplay = config.gameplay;
    if !(8..=256).contains(&config.chunk_size_m)
        || !(1..=16).contains(&config.streaming_radius_chunks)
        || config.tree_spacing_m == 0
        || config.river_spacing_m == 0
        || !config.mountain_height_m.is_finite()
        || config.mountain_height_m <= 0.0
    {
        return Err("world chunk, streaming, and feature settings are invalid".into());
    }
    if !gameplay.day_length_seconds.is_finite()
        || gameplay.day_length_seconds <= 0.0
        || !gameplay.start_hour.is_finite()
        || !(0.0..24.0).contains(&gameplay.start_hour)
        || !gameplay.max_health_units.is_finite()
        || gameplay.max_health_units <= 0.0
        || !gameplay.max_stamina_units.is_finite()
        || gameplay.max_stamina_units <= 0.0
        || !gameplay.max_food_units.is_finite()
        || gameplay.max_food_units <= 0.0
        || !gameplay.food_depletion_seconds.is_finite()
        || gameplay.food_depletion_seconds < 0.0
        || !gameplay.sprint_stamina_per_second.is_finite()
        || gameplay.sprint_stamina_per_second < 0.0
        || !gameplay.jump_stamina_cost_units.is_finite()
        || gameplay.jump_stamina_cost_units < 0.0
        || !gameplay.stamina_regen_per_second.is_finite()
        || gameplay.stamina_regen_per_second < 0.0
        || !gameplay.food_per_stamina_unit.is_finite()
        || gameplay.food_per_stamina_unit < 0.0
        || !gameplay.health_regen_per_second.is_finite()
        || gameplay.health_regen_per_second < 0.0
        || !gameplay.health_regen_food_per_second.is_finite()
        || gameplay.health_regen_food_per_second < 0.0
        || !gameplay.starvation_damage_per_second.is_finite()
        || gameplay.starvation_damage_per_second < 0.0
        || !gameplay.fall_safe_distance_m.is_finite()
        || gameplay.fall_safe_distance_m < 0.0
        || !gameplay.fall_damage_per_meter.is_finite()
        || gameplay.fall_damage_per_meter < 0.0
    {
        return Err("gameplay settings must be finite and within their valid ranges".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::Duration;

    static TEST_CACHE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestCacheDir(PathBuf);

    impl TestCacheDir {
        fn new() -> Self {
            let sequence = TEST_CACHE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "eclipse-world-test-{}-{sequence}",
                std::process::id()
            )))
        }
    }

    impl Drop for TestCacheDir {
        fn drop(&mut self) {
            if self.0.exists() {
                fs::remove_dir_all(&self.0).expect("temporary world cache should be removable");
            }
        }
    }

    fn config() -> WorldConfig {
        let mut config = load_config(include_str!("world.cfg")).unwrap();
        config.chunk_size_m = 16;
        config.streaming_radius_chunks = 1;
        config
    }

    fn test_world(config: WorldConfig) -> (TestCacheDir, World) {
        let directory = TestCacheDir::new();
        let (cache, writer) = ChunkCache::new(directory.0.clone()).unwrap();
        let world = World::new_with_cache(config, cache, writer).unwrap();
        (directory, world)
    }

    fn wait_for_region(world: &mut World, position: Vec3) {
        let center = chunk_coord_at(position.x, position.z, world.config.chunk_size_m).unwrap();
        let desired = desired_chunks(center, world.config.streaming_radius_chunks);
        for _ in 0..1_000 {
            let previous = world.chunks.keys().copied().collect::<Vec<_>>();
            world.update_streaming(position).unwrap();
            let new_chunks = world
                .chunks
                .keys()
                .filter(|coord| !previous.contains(coord))
                .count();
            assert!(new_chunks <= MAX_CHUNKS_PER_FRAME);
            if desired.iter().all(|coord| world.chunks.contains_key(coord))
                && world.streaming_result.is_none()
            {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("streaming worker did not load the active chunk ring in time");
    }

    #[test]
    fn config_loads_gameplay_tuning_and_rejects_invalid_values() {
        let source = include_str!("world.cfg");
        let custom = source.replace("max_health_units=100", "max_health_units=125");
        let config = load_config(&custom).unwrap();
        assert_eq!(config.gameplay.max_health_units, 125.0);
        assert_eq!(config.gameplay.day_length_seconds, 1_800.0);

        let invalid = source.replace("day_length_seconds=1800", "day_length_seconds=0");
        assert!(load_config(&invalid).is_err());
    }

    #[test]
    fn world_positions_use_floor_for_negative_chunk_coordinates() {
        assert_eq!(
            chunk_coord_at(-0.1, -64.0, 64),
            Some(ChunkCoord { x: -1, z: -1 })
        );
        assert_eq!(
            chunk_coord_at(64.0, -64.1, 64),
            Some(ChunkCoord { x: 1, z: -2 })
        );
        assert_eq!(chunk_coord_at(f32::NAN, 0.0, 64), None);
        assert_eq!(chunk_coord_at(f32::MAX, 0.0, 8), None);
    }

    #[test]
    fn configured_streaming_area_matches_the_world_config() {
        let configured = load_config(include_str!("world.cfg")).unwrap();
        let release_config = load_config(include_str!("../../release/Eclipse/world.cfg")).unwrap();
        assert_eq!(WorldConfig::default().streaming_radius_chunks, 4);
        assert_eq!(release_config, configured);
        assert!(validate_config(&configured).is_ok());
        assert!(validate_config(&WorldConfig {
            streaming_radius_chunks: 17,
            ..configured
        })
        .is_err());

        let desired = desired_chunks(
            ChunkCoord { x: -2, z: 5 },
            configured.streaming_radius_chunks,
        );
        let diameter = (configured.streaming_radius_chunks * 2 + 1) as usize;
        assert_eq!(desired.len(), diameter * diameter);
        assert_eq!(desired[0], ChunkCoord { x: -2, z: 5 });
        assert!(desired
            .windows(2)
            .all(|pair| chunk_distance(pair[0], ChunkCoord { x: -2, z: 5 })
                <= chunk_distance(pair[1], ChunkCoord { x: -2, z: 5 })));
    }

    #[test]
    fn streams_rings_with_a_per_frame_budget_and_retains_a_chunk_pool() {
        let (_cache_dir, mut world) = test_world(config());
        assert_eq!(world.chunks.len(), 4);
        let spawn = world.spawn_position();
        assert!(spawn.x.abs() <= 3.0 && spawn.z.abs() <= 3.0);
        assert_eq!(spawn.y, terrain::height_at(world.config, spawn.x, spawn.z));
        assert!(world.is_safe_spawn_ground(spawn.x, spawn.z));
        assert!(crate::river::surface_at(world.config, spawn.x, spawn.z).is_none());
        assert!(!crate::river::is_river(world.config, spawn.x, spawn.z, 2.0));
        assert!(!crate::ravine::is_ravine(
            world.config,
            spawn.x,
            spawn.z,
            2.0
        ));
        assert!(!crate::ocean::is_ocean(world.config, spawn.x, spawn.z));
        wait_for_region(&mut world, spawn);
        assert_eq!(world.chunks.len(), 9);

        let next_chunk_position = Vec3::new(
            world.config.chunk_size_m as f32 * 2.5,
            0.0,
            world.config.chunk_size_m as f32 * 0.5,
        );
        wait_for_region(&mut world, next_chunk_position);
        let next_center = chunk_coord_at(
            next_chunk_position.x,
            next_chunk_position.z,
            world.config.chunk_size_m,
        )
        .unwrap();
        let next_desired = desired_chunks(next_center, world.config.streaming_radius_chunks);
        assert!(next_desired
            .iter()
            .all(|coord| world.chunks.contains_key(coord)));
        assert!(world.chunks.contains_key(&ChunkCoord { x: -1, z: 0 }));
        assert!(world.chunks.len() >= next_desired.len());
        assert!(world
            .raycast_ground(
                Ray {
                    origin: [next_chunk_position.x, 120.0, next_chunk_position.z],
                    direction: [0.0, -1.0, 0.0],
                },
                200.0,
            )
            .is_some());

        let distant_position = Vec3::new(
            world.config.chunk_size_m as f32 * 20.5,
            0.0,
            world.config.chunk_size_m as f32 * 0.5,
        );
        world.update_streaming(distant_position).unwrap();
        assert!(!world.chunks.contains_key(&ChunkCoord { x: -1, z: 0 }));
        assert!(!world.chunks.reusable.is_empty());
        wait_for_region(&mut world, distant_position);
        assert!(!world.chunks.contains_key(&ChunkCoord { x: -1, z: 0 }));
        assert!(desired_chunks(
            chunk_coord_at(
                distant_position.x,
                distant_position.z,
                world.config.chunk_size_m
            )
            .unwrap(),
            world.config.streaming_radius_chunks,
        )
        .iter()
        .all(|coord| world.chunks.contains_key(coord)));

        let first = terrain::generate_chunk(config(), ChunkCoord { x: -7, z: 13 });
        let second = terrain::generate_chunk(config(), ChunkCoord { x: -7, z: 13 });
        assert_eq!(
            first
                .vertices
                .iter()
                .map(|vertex| vertex.position)
                .collect::<Vec<_>>(),
            second
                .vertices
                .iter()
                .map(|vertex| vertex.position)
                .collect::<Vec<_>>()
        );
        assert_eq!(first.indices, second.indices);
    }

    #[test]
    fn spawn_is_deterministic_near_origin_on_supported_terrain() {
        let mut spawn_biomes = std::collections::HashSet::new();
        for seed in [0, 1, 414_165, 454_138, 12_204_589, u64::MAX] {
            let world_config = WorldConfig { seed, ..config() };
            let (_cache_dir, world) = test_world(world_config);
            let spawn = world.spawn_position();

            assert_eq!(spawn, world.spawn_position());
            assert!(spawn.x.abs() <= 3.0 && spawn.z.abs() <= 3.0);
            assert_eq!(spawn.y, terrain::height_at(world.config, spawn.x, spawn.z));
            assert!(world
                .raycast_ground(
                    Ray {
                        origin: [spawn.x, spawn.y + 16.0, spawn.z],
                        direction: [0.0, -1.0, 0.0],
                    },
                    32.0,
                )
                .is_some());
            spawn_biomes.insert(terrain::biome_at(world.config, spawn.x, spawn.z));
        }
        assert!(
            spawn_biomes.len() > 1,
            "changing the seed should also change the spawn biome"
        );
    }

    #[test]
    fn steep_terrain_is_rejected_for_spawn() {
        let world_config = WorldConfig {
            mountain_height_m: 220.0,
            ..config()
        };
        let steep = (-4_000..=4_000)
            .step_by(16)
            .flat_map(|z| {
                (-4_000..=4_000)
                    .step_by(16)
                    .map(move |x| (x as f32, z as f32))
            })
            .find(|(x, z)| terrain::ground_slope(world_config, *x, *z) > 0.7)
            .expect("generated world should include steep terrain");
        let (_cache_dir, world) = test_world(world_config);

        assert!(!world.is_safe_spawn_ground(steep.0, steep.1));
    }

    #[test]
    fn combines_chunk_draw_ranges_without_index_overlap() {
        let mut world_config = config();
        world_config.chunk_size_m = 64;
        let river_z = (-120..=120)
            .find(|z| crate::river::is_river(world_config, 16.0, *z as f32, 0.0))
            .expect("generated world should contain a river");
        let (_cache_dir, mut world) = test_world(world_config);
        let position = Vec3::new(
            world.config.chunk_size_m as f32 * 0.5,
            0.0,
            river_z as f32 + 0.5,
        );
        wait_for_region(&mut world, position);
        assert_eq!(world.terrain.chunks.len(), 9);
        assert!(world.terrain.chunks.iter().all(|chunk| {
            !chunk.indices.is_empty() && chunk.indices.end <= world.terrain.indices.len()
        }));
        assert!(!world.water.indices.is_empty());
    }
}
