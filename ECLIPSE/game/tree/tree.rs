use crate::world::{ChunkCoord, WorldConfig};
use eclipse_engine::asset_pack::AssetPack;
use eclipse_engine::collision::CylinderCollider;
use eclipse_engine::mesh::{ChunkedMesh, InstanceTransform};
use eclipse_engine::model_loader::ModelLoader;
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

const TREE_MODEL_FILE: &str = "tree.glb";
const TRUNK_RADIUS_M: f32 = 0.23;
const TRUNK_HEIGHT_M: f32 = 1.85;
const MAX_TREE_SCALE: f32 = 1.24;

#[derive(Debug)]
pub struct TreeForest {
    pub instances: Vec<InstanceTransform>,
    collision_by_chunk: HashMap<ChunkCoord, Vec<CylinderCollider>>,
    chunk_size_m: f32,
}

#[derive(Debug, Default)]
pub(crate) struct TreeChunk {
    pub instances: Vec<InstanceTransform>,
    pub colliders: Vec<CylinderCollider>,
}

impl TreeForest {
    #[cfg(test)]
    pub fn generate(
        config: WorldConfig,
        active_chunks: &[ChunkCoord],
        height_at: fn(WorldConfig, f32, f32) -> f32,
    ) -> Self {
        if active_chunks.is_empty() {
            return Self {
                instances: Vec::new(),
                collision_by_chunk: HashMap::new(),
                chunk_size_m: config.chunk_size_m as f32,
            };
        }
        let worker_count = std::thread::available_parallelism()
            .map(|parallelism| parallelism.get())
            .unwrap_or(2)
            .max(2)
            .min(8)
            .min(active_chunks.len().max(1));
        let mut chunk_trees = std::thread::scope(|scope| {
            let handles = (0..worker_count)
                .map(|worker_index| {
                    scope.spawn(move || {
                        active_chunks
                            .iter()
                            .copied()
                            .enumerate()
                            .skip(worker_index)
                            .step_by(worker_count)
                            .map(|(index, chunk)| {
                                let (instances, colliders) =
                                    generate_chunk_trees(config, chunk, height_at);
                                (index, chunk, instances, colliders)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().expect("tree placement worker panicked"))
                .collect::<Vec<_>>()
        });
        chunk_trees.sort_unstable_by_key(|(index, _, _, _)| *index);

        let mut instances = Vec::new();
        let mut collision_by_chunk: HashMap<ChunkCoord, Vec<CylinderCollider>> = HashMap::new();
        for (_, chunk, chunk_instances, colliders) in chunk_trees {
            instances.extend(chunk_instances);
            if !colliders.is_empty() {
                collision_by_chunk.insert(chunk, colliders);
            }
        }

        Self {
            instances,
            collision_by_chunk,
            chunk_size_m: config.chunk_size_m as f32,
        }
    }

    pub fn intersects(&self, collider: &CylinderCollider) -> bool {
        let search_radius = collider.radius + TRUNK_RADIUS_M * MAX_TREE_SCALE;
        let chunk_size = self.chunk_size_m;
        let min_x = ((collider.base[0] - search_radius) / chunk_size).floor() as i32;
        let max_x = ((collider.base[0] + search_radius) / chunk_size).floor() as i32;
        let min_z = ((collider.base[2] - search_radius) / chunk_size).floor() as i32;
        let max_z = ((collider.base[2] + search_radius) / chunk_size).floor() as i32;
        (min_z..=max_z).any(|z| {
            (min_x..=max_x).any(|x| {
                self.collision_by_chunk
                    .get(&ChunkCoord { x, z })
                    .map(|trees| trees.iter().any(|tree| collider.intersects(tree)))
                    .unwrap_or(false)
            })
        })
    }

    pub(crate) fn generate_chunk(
        config: WorldConfig,
        chunk: ChunkCoord,
        height_at: fn(WorldConfig, f32, f32) -> f32,
    ) -> TreeChunk {
        let (instances, colliders) = generate_chunk_trees(config, chunk, height_at);
        TreeChunk {
            instances,
            colliders,
        }
    }

    pub(crate) fn from_chunks(
        config: WorldConfig,
        chunks: &[(ChunkCoord, Arc<TreeChunk>)],
    ) -> Self {
        let mut instances = Vec::new();
        let mut collision_by_chunk = HashMap::new();
        for (coord, trees) in chunks {
            instances.extend_from_slice(&trees.instances);
            if !trees.colliders.is_empty() {
                collision_by_chunk.insert(*coord, trees.colliders.clone());
            }
        }
        Self {
            instances,
            collision_by_chunk,
            chunk_size_m: config.chunk_size_m as f32,
        }
    }

    pub fn build_mesh() -> Result<ChunkedMesh, Box<dyn Error>> {
        let executable = std::env::current_exe()?;
        let executable_directory = executable
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let packaged_assets = executable_directory
            .parent()
            .unwrap_or(executable_directory)
            .join("assets");
        let packaged = packaged_assets.join("model.pak");
        let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tree")
            .join(TREE_MODEL_FILE);
        if packaged.is_file() {
            let pack = AssetPack::open(packaged)?;
            return ModelLoader::load_glb_from_reader(pack.open_asset(TREE_MODEL_FILE)?);
        }
        if packaged_assets.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "release model pack is missing: {}",
                    packaged_assets.join("model.pak").display()
                ),
            )
            .into());
        }
        ModelLoader::load_glb(source_path)
    }
}

fn generate_chunk_trees(
    config: WorldConfig,
    chunk: ChunkCoord,
    height_at: fn(WorldConfig, f32, f32) -> f32,
) -> (Vec<InstanceTransform>, Vec<CylinderCollider>) {
    let spacing = config.tree_spacing_m as f32;
    let chunk_size = config.chunk_size_m as f32;
    let origin_x = chunk.x as f32 * chunk_size;
    let origin_z = chunk.z as f32 * chunk_size;
    let first_x = (origin_x / spacing).floor() as i64 - 1;
    let first_z = (origin_z / spacing).floor() as i64 - 1;
    let last_x = ((origin_x + chunk_size) / spacing).ceil() as i64 + 1;
    let last_z = ((origin_z + chunk_size) / spacing).ceil() as i64 + 1;
    let mut instances = Vec::new();
    let mut colliders = Vec::new();

    for grid_z in first_z..=last_z {
        for grid_x in first_x..=last_x {
            let seed = tree_seed(config.seed, grid_x, grid_z);
            let x = (grid_x as f32 + 0.5) * spacing + (tree_random(seed) - 0.5) * spacing * 0.26;
            let z = (grid_z as f32 + 0.5) * spacing
                + (tree_random(seed ^ 0xa511_e9b3) - 0.5) * spacing * 0.26;
            let owner = ChunkCoord {
                x: (x / chunk_size).floor() as i32,
                z: (z / chunk_size).floor() as i32,
            };
            if owner != chunk {
                continue;
            }
            let y = height_at(config, x, z);
            let tree_line = 32.0 + config.mountain_height_m * 0.38;
            if crate::river::is_river(config, x, z, 8.0)
                || crate::ravine::is_ravine(config, x, z, 8.0)
                || crate::ocean::is_ocean(config, x, z)
                || crate::terrain::ground_slope(config, x, z) > 0.55
                || y > tree_line
            {
                continue;
            }
            if tree_random(seed ^ 0x63d8_3595) > crate::terrain::forest_density(config, x, z) {
                continue;
            }

            let scale = 0.82 + tree_random(seed ^ 0x92d6_8ca2) * 0.42;
            instances.push(InstanceTransform {
                position_scale: [x, y, z, scale],
            });
            colliders.push(CylinderCollider {
                base: [x, y, z],
                radius: TRUNK_RADIUS_M * scale,
                height: TRUNK_HEIGHT_M * scale,
            });
        }
    }
    (instances, colliders)
}

fn tree_seed(seed: u64, x: i64, z: i64) -> u32 {
    let mut value = seed ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= (z as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value as u32
}

fn tree_random(mut value: u32) -> f32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846ca68b);
    value ^= value >> 16;

    value as f32 / u32::MAX as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> WorldConfig {
        WorldConfig {
            chunk_size_m: 32,
            streaming_radius_chunks: 2,
            seed: 414165,
            tree_spacing_m: 14,
            river_spacing_m: 320,
            mountain_height_m: 64.0,
            ..WorldConfig::default()
        }
    }

    fn find_populated_forest_chunk(config: WorldConfig) -> ChunkCoord {
        for radius in 0_i32..=128 {
            for z in -radius..=radius {
                for x in -radius..=radius {
                    if radius > 0 && x.abs().max(z.abs()) != radius {
                        continue;
                    }
                    let coord = ChunkCoord { x, z };
                    if !TreeForest::generate(config, &[coord], crate::terrain::height_at)
                        .instances
                        .is_empty()
                    {
                        return coord;
                    }
                }
            }
        }
        panic!("generated world should include a populated forest chunk");
    }

    #[test]
    fn assigned_glb_loads_as_a_colored_static_tree_mesh() {
        let mesh = TreeForest::build_mesh().unwrap();
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());
        assert!(mesh
            .indices
            .iter()
            .all(|index| (*index as usize) < mesh.vertices.len()));
        assert!(mesh.vertices.iter().any(|vertex| vertex.position[1] > 6.0));
        assert!(mesh.vertices.iter().all(|vertex| {
            vertex.position.iter().all(|value| value.is_finite())
                && vertex.normal.iter().all(|value| value.is_finite())
                && vertex.color.iter().all(|value| value.is_finite())
        }));
        assert!(mesh
            .vertices
            .iter()
            .any(|vertex| vertex.color[1] > vertex.color[0]));
    }

    #[test]
    fn tree_model_can_be_loaded_from_a_model_pack() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tree")
            .join(TREE_MODEL_FILE);
        let temp_dir =
            std::env::temp_dir().join(format!("eclipse-model-pack-test-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pack_path = temp_dir.join("model.pak");

        let mut builder = eclipse_engine::asset_pack::AssetPackBuilder::default();
        builder.add_file(TREE_MODEL_FILE, source).unwrap();
        builder.write_to(&pack_path).unwrap();

        let pack = AssetPack::open(&pack_path).unwrap();
        let mesh =
            ModelLoader::load_glb_from_reader(pack.open_asset(TREE_MODEL_FILE).unwrap()).unwrap();
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn tree_collision_checks_trunks_but_not_foliage() {
        let chunk = find_populated_forest_chunk(config());
        let forest = TreeForest::generate(config(), &[chunk], crate::terrain::height_at);
        assert!(!forest.instances.is_empty());
        let tree = forest.instances[0].position_scale;
        let ground = CylinderCollider {
            base: [tree[0], tree[1], tree[2]],
            radius: 0.35,
            height: 1.8,
        };
        let foliage = CylinderCollider {
            base: [tree[0], tree[1] + 2.0, tree[2]],
            radius: 0.35,
            height: 1.0,
        };

        assert!(forest.intersects(&ground));
        assert!(!forest.intersects(&foliage));
    }

    #[test]
    fn generated_trees_stay_off_river_banks_and_steep_high_slopes() {
        let config = config();
        let center = find_populated_forest_chunk(config);
        let chunks = (-2..=2)
            .flat_map(|z| {
                (-2..=2).map(move |x| ChunkCoord {
                    x: center.x + x,
                    z: center.z + z,
                })
            })
            .collect::<Vec<_>>();
        let forest = TreeForest::generate(config, &chunks, crate::terrain::height_at);
        let tree_line = 32.0 + config.mountain_height_m * 0.38;

        assert!(!forest.instances.is_empty());
        assert!(forest.instances.iter().all(|tree| {
            let [x, y, z, _] = tree.position_scale;
            !crate::river::is_river(config, x, z, 8.0)
                && !crate::ravine::is_ravine(config, x, z, 8.0)
                && !crate::ocean::is_ocean(config, x, z)
                && crate::terrain::ground_slope(config, x, z) <= 0.55
                && y <= tree_line
        }));
    }
}
