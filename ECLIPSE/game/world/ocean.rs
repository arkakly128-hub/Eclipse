use crate::terrain;
use crate::world::{ChunkCoord, WorldConfig};
use eclipse_engine::mesh::{ChunkedMesh, MeshChunk, MeshVertex};

pub const SEA_LEVEL: f32 = 8.0;
const OCEAN_FLOOR: f32 = -18.0;

#[cfg(test)]
pub fn generate_chunk(config: WorldConfig, coord: ChunkCoord) -> ChunkedMesh {
    let mut mesh = ChunkedMesh::default();
    generate_chunk_into(config, coord, &mut mesh);
    mesh
}

pub(crate) fn generate_chunk_into(config: WorldConfig, coord: ChunkCoord, mesh: &mut ChunkedMesh) {
    let cells = config.chunk_size_m as usize;
    let origin_x = coord.x as i64 * config.chunk_size_m as i64;
    let origin_z = coord.z as i64 * config.chunk_size_m as i64;
    mesh.vertices.clear();
    mesh.indices.clear();
    mesh.chunks.clear();
    mesh.vertices.reserve(cells * cells * 2);
    mesh.indices.reserve(cells * cells * 6);
    mesh.chunks.reserve(1);

    for local_z in 0..cells {
        for local_x in 0..cells {
            let width = 1.0;
            let depth = 1.0;
            let x = origin_x as f32 + local_x as f32;
            let z = origin_z as f32 + local_z as f32;
            if !is_ocean(config, x + width * 0.5, z + depth * 0.5) {
                continue;
            }
            let positions = [
                [x, z],
                [x, z + depth],
                [x + width, z],
                [x + width, z + depth],
            ];
            let samples = positions.map(|[px, pz]| {
                let height = terrain::height_at(config, px, pz);
                let depth = if is_ocean(config, px, pz) {
                    SEA_LEVEL - height
                } else {
                    (SEA_LEVEL - height).min(0.0)
                };
                [px, SEA_LEVEL, pz, depth]
            });
            let has_ocean_corner = samples.iter().any(|sample| sample[3] > 0.0);
            if !has_ocean_corner {
                continue;
            }

            for triangle in [
                [samples[0], samples[1], samples[2]],
                [samples[2], samples[1], samples[3]],
            ] {
                let (polygon, count) = crate::water_waves::clip_triangle(
                    triangle,
                    crate::water_waves::MAX_WATER_WAVE_HEIGHT,
                );
                if count < 3 {
                    continue;
                }

                let base = mesh.vertices.len() as u32;
                for [px, py, pz, water_depth] in polygon[..count].iter().copied() {
                    let depth_factor = crate::water_waves::depth_factor(
                        water_depth,
                        crate::water_waves::OCEAN_STYLE,
                    );
                    mesh.vertices.push(MeshVertex {
                        position: [px, py, pz],
                        normal: [0.0, 1.0, 0.0],
                        color: [depth_factor, water_depth, 0.0],
                        texture_data: [px, pz, crate::water_waves::OCEAN_STYLE],
                    });
                }
                for index in 1..count - 1 {
                    mesh.indices.extend_from_slice(&[
                        base,
                        base + index as u32,
                        base + index as u32 + 1,
                    ]);
                }
            }
        }
    }

    if !mesh.indices.is_empty() {
        mesh.chunks.push(MeshChunk {
            origin: [origin_x, origin_z],
            indices: 0..mesh.indices.len(),
        });
    }
}

pub fn is_ocean(config: WorldConfig, x: f32, z: f32) -> bool {
    let (continent, height) = terrain::land_sample(config, x, z);
    is_ocean_sample(continent, height)
}

pub fn shape_land(continent: f32, land_height: f32) -> f32 {
    let land_fraction = terrain::smoothstep(0.19, 0.34, continent);
    OCEAN_FLOOR + (land_height - OCEAN_FLOOR) * land_fraction
}

pub(crate) fn is_ocean_sample(continent: f32, height: f32) -> bool {
    continent < 0.42 && height <= SEA_LEVEL
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

    #[test]
    fn ocean_regions_generate_sea_level_water() {
        let config = config();
        let coord = (-64..64)
            .step_by(2)
            .flat_map(|z| (-64..64).step_by(2).map(move |x| ChunkCoord { x, z }))
            .find(|coord| {
                let center = config.chunk_size_m as f32 * 0.5;
                is_ocean(
                    config,
                    coord.x as f32 * config.chunk_size_m as f32 + center,
                    coord.z as f32 * config.chunk_size_m as f32 + center,
                )
            })
            .expect("generated world should contain ocean regions");
        let center = config.chunk_size_m as f32 * 0.5;
        assert_eq!(
            terrain::biome_at(
                config,
                coord.x as f32 * config.chunk_size_m as f32 + center,
                coord.z as f32 * config.chunk_size_m as f32 + center,
            ),
            terrain::Biome::Ocean
        );
        let ocean = generate_chunk(config, coord);

        assert!(!ocean.indices.is_empty());
        assert!(ocean.vertices.iter().all(|vertex| {
            vertex.position[1] == SEA_LEVEL
                && vertex.texture_data[2] == crate::water_waves::OCEAN_STYLE
        }));
        let highest_water_vertex = ocean
            .indices
            .iter()
            .map(|index| {
                let vertex = &ocean.vertices[*index as usize];
                terrain::height_at(config, vertex.position[0], vertex.position[2])
            })
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            highest_water_vertex <= SEA_LEVEL - crate::water_waves::MAX_WATER_WAVE_HEIGHT + 0.001,
            "water vertex overlaps terrain at height {highest_water_vertex}"
        );
    }
}
