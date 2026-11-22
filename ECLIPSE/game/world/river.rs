use crate::ocean;
use crate::terrain;
use crate::world::{ChunkCoord, WorldConfig};
use eclipse_engine::mesh::{ChunkedMesh, MeshChunk, MeshVertex};

#[cfg(test)]
pub fn generate_chunk(config: WorldConfig, coord: ChunkCoord) -> ChunkedMesh {
    let mut mesh = ChunkedMesh::default();
    generate_chunk_into(config, coord, &mut mesh);
    mesh
}

pub(crate) fn generate_chunk_into(config: WorldConfig, coord: ChunkCoord, mesh: &mut ChunkedMesh) {
    let cells = config.chunk_size_m as usize;
    let half_meter_cells = cells * 2;
    let origin_x = coord.x as i64 * config.chunk_size_m as i64;
    let origin_z = coord.z as i64 * config.chunk_size_m as i64;
    mesh.vertices.clear();
    mesh.indices.clear();
    mesh.chunks.clear();
    mesh.vertices.reserve(cells * cells * 2);
    mesh.indices.reserve(cells * cells * 6);
    mesh.chunks.reserve(1);

    for local_z in 0..half_meter_cells {
        for local_x in 0..half_meter_cells {
            let x = origin_x as f32 + local_x as f32 * 0.5;
            let z = origin_z as f32 + local_z as f32 * 0.5;
            let center_x = x + 0.25;
            let center_z = z + 0.25;
            if river_distance(config, center_x, center_z) > river_width(config, center_z) + 10.0 {
                continue;
            }
            if ocean::is_ocean(config, center_x, center_z) {
                continue;
            }
            let center_surface = water_level(config, center_x, center_z) + 0.06;
            if terrain::height_at(config, center_x, center_z) >= center_surface {
                continue;
            }

            let positions = [[x, z], [x, z + 0.5], [x + 0.5, z], [x + 0.5, z + 0.5]];
            let samples = positions.map(|[px, pz]| {
                let surface = water_level(config, px, pz) + 0.06;
                let ground = terrain::height_at(config, px, pz);
                let in_channel = is_river(config, px, pz, 10.0) && !ocean::is_ocean(config, px, pz);
                let depth = if in_channel { surface - ground } else { -1.0 };
                [px, surface, pz, depth]
            });
            if !samples.iter().any(|sample| sample[3] > 0.0) {
                continue;
            }

            for triangle in [
                [samples[0], samples[1], samples[2]],
                [samples[2], samples[1], samples[3]],
            ] {
                let (polygon, count) = crate::water_waves::clip_triangle(
                    triangle,
                    crate::water_waves::MAX_WATER_WAVE_HEIGHT + 0.02,
                );
                if count < 3 {
                    continue;
                }

                let base = mesh.vertices.len() as u32;
                for [px, py, pz, water_depth] in polygon[..count].iter().copied() {
                    let depth_factor = crate::water_waves::depth_factor(
                        water_depth,
                        crate::water_waves::RIVER_STYLE,
                    );
                    mesh.vertices.push(MeshVertex {
                        position: [px, py, pz],
                        normal: [0.0, 1.0, 0.0],
                        color: [depth_factor, water_depth, 0.0],
                        texture_data: [px, pz, crate::water_waves::RIVER_STYLE],
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

pub fn carve_terrain(config: WorldConfig, x: f32, z: f32, base_height: f32, is_ocean: bool) -> f32 {
    if is_ocean {
        return base_height;
    }

    let river_level = water_level(config, x, z) - 3.0;
    let width = river_width(config, z);
    let distance = river_distance(config, x, z);
    let valley = 1.0 - terrain::smoothstep(width, width + 10.0, distance);
    base_height + (river_level - base_height) * valley
}

pub fn is_river(config: WorldConfig, x: f32, z: f32, bank_margin: f32) -> bool {
    river_distance(config, x, z) <= river_width(config, z) + bank_margin
}

pub(crate) fn material_weight(config: WorldConfig, x: f32, z: f32) -> f32 {
    1.0 - terrain::smoothstep(
        river_width(config, z),
        river_width(config, z) + 10.0,
        river_distance(config, x, z),
    )
}

pub fn surface_at(config: WorldConfig, x: f32, z: f32) -> Option<f32> {
    let surface = water_level(config, x, z) + 0.06;
    (is_river(config, x, z, 10.0) && terrain::height_at(config, x, z) < surface).then_some(surface)
}

pub(crate) fn water_level(config: WorldConfig, x: f32, z: f32) -> f32 {
    let continent = terrain::continent_noise(config, x, z);
    let raw_height = terrain::raw_base_height(config, x, z, continent);
    (7.0 + continent * 6.0)
        .min(raw_height - 1.5)
        .min(ocean::SEA_LEVEL + 1.0)
        .max(1.0)
}

fn river_distance(config: WorldConfig, x: f32, z: f32) -> f32 {
    let spacing = config.river_spacing_m as f32;
    let band = (z / spacing).round() as i64;
    let phase = terrain::hash_unit(band, 0, config.seed, 0x87c3_7b91) * std::f32::consts::TAU;
    let meander = (x * 0.004 + phase).sin() * 38.0
        + (x * 0.011 + phase * 1.7).sin() * 13.0
        + (terrain::value_noise(x * 0.006, band as f32, config.seed, 0x1646_67b1) - 0.5) * 22.0;
    (z - (band as f32 * spacing + meander)).abs()
}

fn river_width(config: WorldConfig, z: f32) -> f32 {
    let band = (z / config.river_spacing_m as f32).round() as i64;
    3.0 + terrain::hash_unit(band, 0, config.seed, 0xb529_7a4d) * 2.5
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
    fn river_surface_is_separate_from_the_carved_bed() {
        let config = config();
        let river_z: i32 = (-120..=120)
            .find(|z| {
                is_river(config, 16.0, *z as f32, 0.0) && !ocean::is_ocean(config, 16.0, *z as f32)
            })
            .expect("generated region should contain a river");
        assert_eq!(
            terrain::biome_at(config, 16.0, river_z as f32),
            terrain::Biome::River
        );
        let coord = ChunkCoord {
            x: 0,
            z: river_z.div_euclid(config.chunk_size_m as i32),
        };
        let water = generate_chunk(config, coord);
        assert!(!water.indices.is_empty());
        assert!(water
            .vertices
            .iter()
            .all(|vertex| vertex.texture_data[2] == crate::water_waves::RIVER_STYLE));
        assert!(water.indices.len() > (config.chunk_size_m as usize).pow(2) * 6);
        let overlap = water.indices.iter().find_map(|index| {
            let vertex = &water.vertices[*index as usize];
            let ground = terrain::height_at(config, vertex.position[0], vertex.position[2]);
            (ground > vertex.position[1] - crate::water_waves::MAX_WATER_WAVE_HEIGHT + 0.0001)
                .then_some((vertex.position, ground))
        });
        assert!(
            overlap.is_none(),
            "river water overlaps terrain at {overlap:?}"
        );
    }
}
