use crate::world::{ChunkCoord, WorldConfig};
use eclipse_engine::mesh::{ChunkedMesh, MeshChunk, MeshVertex};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Biome {
    Ocean,
    Beach,
    River,
    Ravine,
    Plains,
    Forest,
    Desert,
    MidHills,
    HighHills,
    SnowyPlains,
    SnowyMountains,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
enum TerrainMaterial {
    Grass,
    Forest,
    Mud,
    Rock,
    Sand,
    Snow,
}

impl TerrainMaterial {
    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BiomeWeights {
    plains: f32,
    forest: f32,
    desert: f32,
    mid_hills: f32,
    high_hills: f32,
    snowy_plains: f32,
    snowy_mountains: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct TerrainSample {
    pub height: f32,
    pub normal: [f32; 3],
    pub material: [f32; 3],
}

#[cfg(test)]
pub fn generate_chunk(config: WorldConfig, coord: ChunkCoord) -> ChunkedMesh {
    let samples = generate_samples(config, coord);
    assemble_chunk(config, coord, &samples)
}

pub(crate) fn generate_samples(config: WorldConfig, coord: ChunkCoord) -> Vec<TerrainSample> {
    let cells = config.chunk_size_m as usize;
    let side = cells + 1;
    let halo_side = side + 2;
    let origin_x = coord.x as i64 * config.chunk_size_m as i64;
    let origin_z = coord.z as i64 * config.chunk_size_m as i64;
    let mut heights = Vec::with_capacity(halo_side * halo_side);
    for local_z in 0..halo_side {
        for local_x in 0..halo_side {
            let x = origin_x + local_x as i64 - 1;
            let z = origin_z + local_z as i64 - 1;
            heights.push(height_at(config, x as f32, z as f32));
        }
    }

    let mut samples = Vec::with_capacity(side * side);
    for local_z in 0..side {
        for local_x in 0..side {
            let x = origin_x + local_x as i64;
            let z = origin_z + local_z as i64;
            let height_index = (local_z + 1) * halo_side + local_x + 1;
            let slope_x = heights[height_index + 1] - heights[height_index - 1];
            let slope_z = heights[height_index + halo_side] - heights[height_index - halo_side];
            let inverse_length = (slope_x * slope_x + slope_z * slope_z + 4.0).sqrt().recip();
            samples.push(TerrainSample {
                height: heights[height_index],
                normal: [
                    -slope_x * inverse_length,
                    2.0 * inverse_length,
                    -slope_z * inverse_length,
                ],
                material: terrain_material(config, x as f32, z as f32, heights[height_index]),
            });
        }
    }
    samples
}

#[cfg(test)]
pub(crate) fn assemble_chunk(
    config: WorldConfig,
    coord: ChunkCoord,
    samples: &[TerrainSample],
) -> ChunkedMesh {
    let mut terrain = ChunkedMesh::default();
    assemble_chunk_into(config, coord, samples, &mut terrain);
    terrain
}

pub(crate) fn assemble_chunk_into(
    config: WorldConfig,
    coord: ChunkCoord,
    samples: &[TerrainSample],
    terrain: &mut ChunkedMesh,
) {
    let cells = config.chunk_size_m as usize;
    let side = cells + 1;
    debug_assert_eq!(samples.len(), side * side);
    let first_index = 0;
    let origin_x = coord.x as i64 * config.chunk_size_m as i64;
    let origin_z = coord.z as i64 * config.chunk_size_m as i64;
    terrain.vertices.clear();
    terrain.indices.clear();
    terrain.chunks.clear();
    terrain.vertices.reserve(side * side + 64);
    terrain.indices.reserve(cells * cells * 6 + 384);
    terrain.chunks.reserve(1);

    for (local_z, row) in samples.chunks_exact(side).enumerate() {
        for (local_x, sample) in row.iter().enumerate() {
            let x = origin_x + local_x as i64;
            let z = origin_z + local_z as i64;
            terrain.vertices.push(MeshVertex {
                position: [x as f32, sample.height, z as f32],
                normal: sample.normal,
                color: sample.material,
                texture_data: [x as f32, z as f32, 1.0],
            });
        }
    }

    for local_z in 0..cells {
        for local_x in 0..cells {
            let top_left = (local_z * side + local_x) as u32;
            let top_right = top_left + 1;
            let bottom_left = top_left + side as u32;
            let bottom_right = bottom_left + 1;
            terrain.indices.extend_from_slice(&[
                top_left,
                bottom_left,
                top_right,
                top_right,
                bottom_left,
                bottom_right,
            ]);
        }
    }

    append_cabin_if_selected(terrain, config, coord);
    terrain.chunks.push(MeshChunk {
        origin: [origin_x, origin_z],
        indices: first_index..terrain.indices.len(),
    });
}

fn terrain_material(config: WorldConfig, x: f32, z: f32, height: f32) -> [f32; 3] {
    let (continent, land_height, biome) = land_sample_with_biomes(config, x, z);
    let snowline = 40.0 + config.mountain_height_m * 0.55;
    let mountain_snow = smoothstep(snowline, snowline + 24.0, height);
    let mut weights = [0.0; 6];
    weights[TerrainMaterial::Grass.index()] = biome.plains;
    weights[TerrainMaterial::Forest.index()] = biome.forest;
    weights[TerrainMaterial::Rock.index()] =
        biome.mid_hills + biome.high_hills + biome.snowy_mountains * (1.0 - mountain_snow);
    weights[TerrainMaterial::Sand.index()] = biome.desert;
    weights[TerrainMaterial::Snow.index()] =
        biome.snowy_plains + biome.snowy_mountains * mountain_snow;

    if crate::ocean::is_ocean_sample(continent, land_height) {
        weights.fill(0.0);
        weights[TerrainMaterial::Sand.index()] = 1.0;
    } else {
        blend_material(
            &mut weights,
            TerrainMaterial::Sand,
            beach_weight(continent, land_height),
        );
        blend_material(
            &mut weights,
            TerrainMaterial::Mud,
            crate::river::material_weight(config, x, z),
        );
        blend_material(
            &mut weights,
            TerrainMaterial::Mud,
            crate::ravine::material_weight(config, x, z),
        );
    }

    let mut primary = 0;
    let mut secondary = 1;
    if weights[secondary] > weights[primary] {
        std::mem::swap(&mut primary, &mut secondary);
    }
    for candidate in 2..weights.len() {
        if weights[candidate] > weights[primary] {
            secondary = primary;
            primary = candidate;
        } else if weights[candidate] > weights[secondary] {
            secondary = candidate;
        }
    }

    let (first_material, second_material, secondary_weight) = if primary < secondary {
        (primary, secondary, weights[secondary])
    } else {
        (secondary, primary, weights[primary])
    };
    let pair_weight = weights[primary] + weights[secondary];
    let blend = if pair_weight > 0.0 {
        secondary_weight / pair_weight
    } else {
        0.0
    };
    [first_material as f32, second_material as f32, blend]
}

fn blend_material(weights: &mut [f32; 6], material: TerrainMaterial, amount: f32) {
    let amount = amount.clamp(0.0, 1.0);
    for weight in weights.iter_mut() {
        *weight *= 1.0 - amount;
    }
    weights[material.index()] += amount;
}

pub fn height_at(config: WorldConfig, x: f32, z: f32) -> f32 {
    let (continent, land_height) = land_sample(config, x, z);
    let is_ocean = crate::ocean::is_ocean_sample(continent, land_height);
    let ravined = crate::ravine::carve_terrain(config, x, z, land_height, is_ocean);
    crate::river::carve_terrain(config, x, z, ravined, is_ocean)
}

pub(crate) fn land_sample(config: WorldConfig, x: f32, z: f32) -> (f32, f32) {
    let (continent, height, _) = land_sample_with_biomes(config, x, z);
    (continent, height)
}

fn land_sample_with_biomes(config: WorldConfig, x: f32, z: f32) -> (f32, f32, BiomeWeights) {
    let (warped_x, warped_z) = domain_warp(config, x, z);
    let continent = continent_noise_warped(config, warped_x, warped_z);
    let biome = biome_weights_warped(config, warped_x, warped_z);
    let raw_height = raw_base_height_warped(config, warped_x, warped_z, continent, biome);
    (
        continent,
        crate::ocean::shape_land(continent, raw_height),
        biome,
    )
}

pub fn biome_at(config: WorldConfig, x: f32, z: f32) -> Biome {
    let (continent, land_height, weights) = land_sample_with_biomes(config, x, z);
    if let Some(feature) = feature_biome(config, x, z, continent, land_height) {
        return feature;
    }

    dominant_biome(weights)
}

fn dominant_biome(weights: BiomeWeights) -> Biome {
    let mut biome = Biome::Plains;
    let mut greatest = weights.plains;
    for (candidate, weight) in [
        (Biome::Forest, weights.forest),
        (Biome::Desert, weights.desert),
        (Biome::MidHills, weights.mid_hills),
        (Biome::HighHills, weights.high_hills),
        (Biome::SnowyPlains, weights.snowy_plains),
        (Biome::SnowyMountains, weights.snowy_mountains),
    ] {
        if weight > greatest {
            biome = candidate;
            greatest = weight;
        }
    }
    biome
}

fn feature_biome(
    config: WorldConfig,
    x: f32,
    z: f32,
    continent: f32,
    land_height: f32,
) -> Option<Biome> {
    if crate::ocean::is_ocean_sample(continent, land_height) {
        Some(Biome::Ocean)
    } else if crate::river::is_river(config, x, z, 5.0) {
        Some(Biome::River)
    } else if crate::ravine::is_ravine(config, x, z, 8.0) {
        Some(Biome::Ravine)
    } else if beach_weight(continent, land_height) > 0.25 {
        Some(Biome::Beach)
    } else {
        None
    }
}

fn beach_weight(continent: f32, land_height: f32) -> f32 {
    let coastline = smoothstep(0.23, 0.34, continent) * (1.0 - smoothstep(0.50, 0.60, continent));
    coastline
        * (1.0
            - smoothstep(
                crate::ocean::SEA_LEVEL + 3.0,
                crate::ocean::SEA_LEVEL + 10.0,
                land_height,
            ))
}

pub(crate) fn continent_noise(config: WorldConfig, x: f32, z: f32) -> f32 {
    let (warped_x, warped_z) = domain_warp(config, x, z);
    continent_noise_warped(config, warped_x, warped_z)
}

fn continent_noise_warped(config: WorldConfig, warped_x: f32, warped_z: f32) -> f32 {
    fractal_noise(
        warped_x * 0.00042,
        warped_z * 0.00042,
        config.seed,
        0x2d99_7a4d,
        3,
    )
}

pub(crate) fn raw_base_height(config: WorldConfig, x: f32, z: f32, continent: f32) -> f32 {
    let (warped_x, warped_z) = domain_warp(config, x, z);
    let biome = biome_weights_warped(config, warped_x, warped_z);
    raw_base_height_warped(config, warped_x, warped_z, continent, biome)
}

fn raw_base_height_warped(
    config: WorldConfig,
    warped_x: f32,
    warped_z: f32,
    continent: f32,
    biome: BiomeWeights,
) -> f32 {
    let hills = fractal_noise(
        warped_x * 0.0055,
        warped_z * 0.0055,
        config.seed,
        0x5c8e_21b7,
        3,
    );
    let mountain_region = value_noise(
        warped_x * 0.00034,
        warped_z * 0.00034,
        config.seed,
        0x9e37_79b9,
    );
    let mountain_shape = fractal_noise(
        warped_x * 0.00145,
        warped_z * 0.00145,
        config.seed,
        0x71d6_7fff,
        4,
    );
    let ridge = 1.0 - (mountain_shape * 2.0 - 1.0).abs();
    let mountain =
        ridge.powf(1.55) * smoothstep(0.42, 0.70, mountain_region) * config.mountain_height_m;
    let base = 4.0 + continent * 26.0;
    let desert_dunes = fractal_noise(
        warped_x * 0.0018,
        warped_z * 0.0018,
        config.seed,
        0x32d4_8a81,
        3,
    );
    let plains_height = base + (hills - 0.5) * 4.0;
    let forest_height = base + (hills - 0.5) * 12.0;
    let desert_height = base * 0.86 + (desert_dunes - 0.5) * 6.0;
    let mid_hills_height = base + (hills - 0.5) * 26.0 + mountain * 0.18;
    let high_hills_height = base + (hills - 0.5) * 40.0 + mountain * 0.55;
    let snowy_plains_height = base + (hills - 0.5) * 4.0;
    let snowy_mountains_height = base + (hills - 0.5) * 30.0 + mountain * 1.28;
    let land_height = biome.plains * plains_height
        + biome.forest * forest_height
        + biome.desert * desert_height
        + biome.mid_hills * mid_hills_height
        + biome.high_hills * high_hills_height
        + biome.snowy_plains * snowy_plains_height
        + biome.snowy_mountains * snowy_mountains_height;
    let beach_height = crate::ocean::SEA_LEVEL + 1.1 + (desert_dunes - 0.5) * 1.2;
    let coast = smoothstep(0.23, 0.34, continent) * (1.0 - smoothstep(0.50, 0.60, continent));
    land_height + (beach_height - land_height) * coast
}

fn domain_warp(config: WorldConfig, x: f32, z: f32) -> (f32, f32) {
    let scale = 0.00085;
    let warp_x = fractal_noise(x * scale, z * scale, config.seed, 0xa24b_3c1d, 3);
    let warp_z = fractal_noise(
        x * scale + 17.3,
        z * scale - 29.1,
        config.seed,
        0x6f75_2e19,
        3,
    );
    (x + (warp_x - 0.5) * 220.0, z + (warp_z - 0.5) * 220.0)
}

#[cfg(test)]
fn biome_weights(config: WorldConfig, x: f32, z: f32) -> BiomeWeights {
    let (warped_x, warped_z) = domain_warp(config, x, z);
    biome_weights_warped(config, warped_x, warped_z)
}

fn biome_weights_warped(config: WorldConfig, warped_x: f32, warped_z: f32) -> BiomeWeights {
    let temperature = fractal_noise(
        warped_x * 0.00020,
        warped_z * 0.00020,
        config.seed,
        0x7d31_ebc9,
        4,
    );
    let moisture = fractal_noise(
        warped_x * 0.00028,
        warped_z * 0.00028,
        config.seed,
        0x39c2_6f15,
        4,
    );
    let mountain_region = value_noise(
        warped_x * 0.00034,
        warped_z * 0.00034,
        config.seed,
        0x9e37_79b9,
    );
    let coldness = 1.0 - smoothstep(0.32, 0.54, temperature);
    let high_share = smoothstep(0.70, 0.86, mountain_region);
    let mid_share = smoothstep(0.34, 0.66, mountain_region) * (1.0 - high_share);
    let warm_share = 1.0 - coldness;
    let lowland_share = warm_share * (1.0 - high_share - mid_share);
    let forest_score = lowland_share
        * smoothstep(0.44, 0.70, moisture)
        * (1.0 - smoothstep(0.72, 0.88, temperature))
        * 0.92;
    let desert_score = lowland_share
        * smoothstep(0.54, 0.78, temperature)
        * (1.0 - smoothstep(0.33, 0.63, moisture))
        * 0.90;
    let specialized = forest_score + desert_score;
    let specialized_scale = if specialized > lowland_share {
        lowland_share / specialized
    } else {
        1.0
    };
    let forest = forest_score * specialized_scale;
    let desert = desert_score * specialized_scale;
    let plains = (lowland_share - forest - desert).max(0.0);
    let mid_hills = warm_share * mid_share;
    let high_hills = warm_share * high_share;
    let snowy_plains = coldness * (1.0 - high_share);
    let snowy_mountains = coldness * high_share;
    let total =
        (plains + forest + desert + mid_hills + high_hills + snowy_plains + snowy_mountains)
            .max(f32::EPSILON);
    BiomeWeights {
        plains: plains / total,
        forest: forest / total,
        desert: desert / total,
        mid_hills: mid_hills / total,
        high_hills: high_hills / total,
        snowy_plains: snowy_plains / total,
        snowy_mountains: snowy_mountains / total,
    }
}

pub fn forest_density(config: WorldConfig, x: f32, z: f32) -> f32 {
    let moisture = value_noise(x * 0.0018, z * 0.0018, config.seed, 0x31a5_4c27);
    let vegetation = smoothstep(0.28, 0.72, moisture);
    match biome_at(config, x, z) {
        Biome::Forest => 0.72 + vegetation * 0.28,
        Biome::Plains => vegetation * 0.16,
        Biome::Desert => vegetation * 0.02,
        Biome::MidHills => vegetation * 0.20,
        Biome::HighHills => vegetation * 0.10,
        Biome::SnowyPlains => vegetation * 0.025,
        Biome::SnowyMountains => vegetation * 0.035,
        Biome::Ocean | Biome::Beach | Biome::River | Biome::Ravine => 0.0,
    }
}

pub fn ground_slope(config: WorldConfig, x: f32, z: f32) -> f32 {
    let slope_x = (height_at(config, x + 1.0, z) - height_at(config, x - 1.0, z)) * 0.5;
    let slope_z = (height_at(config, x, z + 1.0) - height_at(config, x, z - 1.0)) * 0.5;
    (slope_x * slope_x + slope_z * slope_z).sqrt()
}

fn fractal_noise(x: f32, z: f32, seed: u64, salt: u64, octaves: u32) -> f32 {
    let mut frequency = 1.0;
    let mut amplitude = 0.5;
    let mut total = 0.0;
    let mut weight = 0.0;
    for octave in 0..octaves {
        total += value_noise(
            x * frequency,
            z * frequency,
            seed,
            salt.wrapping_add(octave as u64 * 0x9e37_79b9),
        ) * amplitude;
        weight += amplitude;
        frequency *= 2.03;
        amplitude *= 0.5;
    }
    total / weight
}

pub(crate) fn value_noise(x: f32, z: f32, seed: u64, salt: u64) -> f32 {
    let x0 = x.floor() as i64;
    let z0 = z.floor() as i64;
    let blend_x = smooth_curve(x - x.floor());
    let blend_z = smooth_curve(z - z.floor());
    let a = hash_unit(x0, z0, seed, salt);
    let b = hash_unit(x0 + 1, z0, seed, salt);
    let c = hash_unit(x0, z0 + 1, seed, salt);
    let d = hash_unit(x0 + 1, z0 + 1, seed, salt);
    let low = a + (b - a) * blend_x;
    let high = c + (d - c) * blend_x;
    low + (high - low) * blend_z
}

pub(crate) fn hash_unit(x: i64, z: i64, seed: u64, salt: u64) -> f32 {
    let mut value = seed
        ^ salt
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (z as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    (value >> 40) as f32 / (1_u32 << 24) as f32
}

fn smooth_curve(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

pub(crate) fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn append_cabin_if_selected(mesh: &mut ChunkedMesh, config: WorldConfig, coord: ChunkCoord) {
    let selector = hash_unit(coord.x as i64, coord.z as i64, config.seed, 0xd1b5_4a32);
    if selector > 0.055 {
        return;
    }

    let chunk_size = config.chunk_size_m as f32;
    let x = coord.x as f32 * chunk_size + chunk_size * 0.52;
    let z = coord.z as f32 * chunk_size + chunk_size * 0.52;
    if crate::river::is_river(config, x, z, 12.0)
        || crate::ravine::is_ravine(config, x, z, 12.0)
        || crate::ocean::is_ocean(config, x, z)
    {
        return;
    }
    let ground = height_at(config, x, z);
    if ground_slope(config, x, z) > 0.55 {
        return;
    }

    append_cabin(mesh, [x, ground, z]);
}

fn append_cabin(mesh: &mut ChunkedMesh, center: [f32; 3]) {
    let [cx, ground, cz] = center;
    let half_width = 3.0;
    let half_depth = 2.5;
    let wall_height = 2.7;
    let ridge_height = 4.0;
    let x0 = cx - half_width;
    let x1 = cx + half_width;
    let z0 = cz - half_depth;
    let z1 = cz + half_depth;
    let eave = ground + wall_height;
    let ridge = ground + ridge_height;
    let wood = [0.24, 0.12, 0.055];
    let wood_light = [0.34, 0.19, 0.09];
    let roof = [0.12, 0.055, 0.035];
    let door = [0.055, 0.026, 0.013];

    append_quad(
        mesh,
        [
            [x0, ground, z0],
            [cx - 0.65, ground, z0],
            [cx - 0.65, eave, z0],
            [x0, eave, z0],
        ],
        wood,
        [0.0, 0.0, -1.0],
    );
    append_quad(
        mesh,
        [
            [cx + 0.65, ground, z0],
            [x1, ground, z0],
            [x1, eave, z0],
            [cx + 0.65, eave, z0],
        ],
        wood,
        [0.0, 0.0, -1.0],
    );
    append_quad(
        mesh,
        [
            [cx - 0.65, ground + 2.1, z0],
            [cx + 0.65, ground + 2.1, z0],
            [cx + 0.65, eave, z0],
            [cx - 0.65, eave, z0],
        ],
        wood_light,
        [0.0, 0.0, -1.0],
    );
    append_quad(
        mesh,
        [
            [cx - 0.55, ground, z0 - 0.025],
            [cx + 0.55, ground, z0 - 0.025],
            [cx + 0.55, ground + 2.1, z0 - 0.025],
            [cx - 0.55, ground + 2.1, z0 - 0.025],
        ],
        door,
        [0.0, 0.0, -1.0],
    );
    append_quad(
        mesh,
        [
            [x0, ground, z1],
            [x0, eave, z1],
            [x1, eave, z1],
            [x1, ground, z1],
        ],
        wood,
        [0.0, 0.0, 1.0],
    );
    append_quad(
        mesh,
        [
            [x0, ground, z0],
            [x0, eave, z0],
            [x0, eave, z1],
            [x0, ground, z1],
        ],
        wood_light,
        [-1.0, 0.0, 0.0],
    );
    append_quad(
        mesh,
        [
            [x1, ground, z0],
            [x1, ground, z1],
            [x1, eave, z1],
            [x1, eave, z0],
        ],
        wood,
        [1.0, 0.0, 0.0],
    );
    append_triangle(
        mesh,
        [x0, eave, z0],
        [cx, ridge, z0],
        [x1, eave, z0],
        wood_light,
        [0.0, 0.0, -1.0],
    );
    append_triangle(
        mesh,
        [x0, eave, z1],
        [x1, eave, z1],
        [cx, ridge, z1],
        wood_light,
        [0.0, 0.0, 1.0],
    );
    append_quad(
        mesh,
        [
            [x0 - 0.35, eave, z0 - 0.35],
            [cx, ridge, z0 - 0.35],
            [cx, ridge, z1 + 0.35],
            [x0 - 0.35, eave, z1 + 0.35],
        ],
        roof,
        [-0.4, 0.8, 0.0],
    );
    append_quad(
        mesh,
        [
            [cx, ridge, z0 - 0.35],
            [x1 + 0.35, eave, z0 - 0.35],
            [x1 + 0.35, eave, z1 + 0.35],
            [cx, ridge, z1 + 0.35],
        ],
        roof,
        [0.4, 0.8, 0.0],
    );
}

fn append_quad(mesh: &mut ChunkedMesh, points: [[f32; 3]; 4], color: [f32; 3], normal: [f32; 3]) {
    append_triangle(mesh, points[0], points[1], points[2], color, normal);
    append_triangle(mesh, points[0], points[2], points[3], color, normal);
}

fn append_triangle(
    mesh: &mut ChunkedMesh,
    a: [f32; 3],
    mut b: [f32; 3],
    mut c: [f32; 3],
    color: [f32; 3],
    outward: [f32; 3],
) {
    let mut normal = normalize(cross(subtract(b, a), subtract(c, a)));
    if dot(normal, outward) < 0.0 {
        std::mem::swap(&mut b, &mut c);
        normal = normalize(cross(subtract(b, a), subtract(c, a)));
    }
    let base = mesh.vertices.len() as u32;
    for position in [a, b, c] {
        mesh.vertices.push(MeshVertex {
            position,
            normal,
            color,
            texture_data: [position[0], position[2], 0.0],
        });
    }
    mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
}

fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(vector: [f32; 3]) -> [f32; 3] {
    let length = dot(vector, vector).sqrt().max(f32::EPSILON);
    [vector[0] / length, vector[1] / length, vector[2] / length]
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
    fn neighboring_chunks_share_identical_seam_vertices() {
        let config = config();
        let left = generate_chunk(config, ChunkCoord { x: -1, z: 2 });
        let right = generate_chunk(config, ChunkCoord { x: 0, z: 2 });
        let left_side = config.chunk_size_m as usize + 1;
        let left_seam = (0..left_side)
            .map(|row| left.vertices[row * left_side + config.chunk_size_m as usize].position)
            .collect::<Vec<_>>();
        let right_seam = (0..left_side)
            .map(|row| right.vertices[row * left_side].position)
            .collect::<Vec<_>>();

        assert_eq!(left_seam, right_seam);
        let left_samples = (0..left_side)
            .map(|row| &left.vertices[row * left_side + config.chunk_size_m as usize])
            .collect::<Vec<_>>();
        let right_samples = (0..left_side)
            .map(|row| &right.vertices[row * left_side])
            .collect::<Vec<_>>();
        for (left, right) in left_samples.iter().zip(right_samples) {
            assert_eq!(left.normal, right.normal);
            assert_eq!(left.color, right.color);
        }
    }

    #[test]
    fn biome_samples_are_seeded_and_height_blends_are_continuous() {
        let config = config();
        let repeated = biome_at(config, -128.5, 96.25);
        assert_eq!(repeated, biome_at(config, -128.5, 96.25));
        let mut biomes = std::collections::HashSet::new();
        for z in (-5_000..=5_000).step_by(400) {
            for x in (-5_000..=5_000).step_by(400) {
                biomes.insert(biome_at(config, x as f32, z as f32));
            }
        }
        assert!(
            biomes.len() >= 3,
            "biome fields should produce varied regions"
        );

        let mut biome_transitions = 0;
        for z in [-5_000.0, -2_500.0, 0.0, 2_500.0, 5_000.0] {
            let mut previous = dominant_biome(biome_weights(config, -5_000.0, z));
            for x in (-4_900..=5_000).step_by(100) {
                let current = dominant_biome(biome_weights(config, x as f32, z));
                biome_transitions += usize::from(current != previous);
                previous = current;
            }
        }
        assert!(
            biome_transitions <= 100,
            "biomes should form broad regions, not change every few hundred meters"
        );

        for (x, z) in [(-800.0, 400.0), (0.0, 0.0), (600.0, -900.0)] {
            let weights = biome_weights(config, x, z);
            let total = weights.plains
                + weights.forest
                + weights.desert
                + weights.mid_hills
                + weights.high_hills
                + weights.snowy_plains
                + weights.snowy_mountains;
            assert!((total - 1.0).abs() < 0.0001);
        }

        let mut border = None;
        'search: for z in (-5_000..=5_000).step_by(100) {
            for x in (-5_000..=5_000).step_by(100) {
                let first = biome_at(config, x as f32, z as f32);
                let second = biome_at(config, x as f32 + 1.0, z as f32);
                if first != second {
                    border = Some((x as f32, z as f32));
                    break 'search;
                }
            }
        }
        let (x, z) = border.expect("seeded biomes should have blended borders");
        let left = height_at(config, x + 0.49, z);
        let right = height_at(config, x + 0.51, z);
        assert!((left - right).abs() < 0.2);
    }

    #[test]
    fn seeded_biomes_include_distinct_landscapes_and_coastal_beaches() {
        let config = WorldConfig {
            mountain_height_m: 220.0,
            ..config()
        };
        let mut biomes = std::collections::HashSet::new();
        let mut beach = None;
        let mut forest = None;
        let mut plains = None;
        let mut snowy_plains = None;
        let mut snowy_mountains: Option<(f32, f32, f32)> = None;
        let mut highest_terrain = f32::NEG_INFINITY;
        for z in (-40_000..=40_000).step_by(400) {
            for x in (-40_000..=40_000).step_by(400) {
                let biome = biome_at(config, x as f32, z as f32);
                biomes.insert(dominant_biome(biome_weights(config, x as f32, z as f32)));
                let height = height_at(config, x as f32, z as f32);
                highest_terrain = highest_terrain.max(height);
                if biome == Biome::Beach {
                    beach = Some((x as f32, z as f32));
                } else if biome == Biome::Forest {
                    forest = Some((x as f32, z as f32));
                } else if biome == Biome::Plains {
                    plains = Some((x as f32, z as f32));
                } else if biome == Biome::SnowyPlains {
                    snowy_plains.get_or_insert((x as f32, z as f32));
                } else if biome == Biome::SnowyMountains
                    && snowy_mountains
                        .map(|(_, _, best_height)| height > best_height)
                        .unwrap_or(true)
                {
                    snowy_mountains = Some((x as f32, z as f32, height));
                }
            }
        }

        for biome in [
            Biome::Plains,
            Biome::Forest,
            Biome::Desert,
            Biome::MidHills,
            Biome::HighHills,
            Biome::SnowyPlains,
            Biome::SnowyMountains,
        ] {
            assert!(biomes.contains(&biome), "missing generated biome {biome:?}");
        }
        assert!(
            highest_terrain > 100.0,
            "snowy mountain regions should generate substantial peaks"
        );

        let (forest_x, forest_z) = forest.expect("seeded world should contain forests");
        let (plains_x, plains_z) = plains.expect("seeded world should contain plains");
        assert!(
            forest_density(config, forest_x, forest_z) > forest_density(config, plains_x, plains_z),
            "forests should have denser tree coverage than plains"
        );
        let forest_material = terrain_material(
            config,
            forest_x,
            forest_z,
            height_at(config, forest_x, forest_z),
        );
        let plains_material = terrain_material(
            config,
            plains_x,
            plains_z,
            height_at(config, plains_x, plains_z),
        );
        assert!(forest_material[..2].contains(&(TerrainMaterial::Forest.index() as f32)));
        assert!(plains_material[..2].contains(&(TerrainMaterial::Grass.index() as f32)));
        let (snow_plain_x, snow_plain_z) =
            snowy_plains.expect("seeded world should contain snowy plains");
        let snow_plain_material = terrain_material(
            config,
            snow_plain_x,
            snow_plain_z,
            height_at(config, snow_plain_x, snow_plain_z),
        );
        assert!(snow_plain_material[..2].contains(&(TerrainMaterial::Snow.index() as f32)));
        let (snow_mountain_x, snow_mountain_z, snow_mountain_height) =
            snowy_mountains.expect("seeded world should contain snowy mountains");
        let snow_mountain_material = terrain_material(
            config,
            snow_mountain_x,
            snow_mountain_z,
            snow_mountain_height,
        );
        assert!(snow_mountain_material[..2].contains(&(TerrainMaterial::Snow.index() as f32)));

        let (beach_x, beach_z) = beach.expect("seeded world should contain beaches");
        let beach_material = terrain_material(
            config,
            beach_x,
            beach_z,
            height_at(config, beach_x, beach_z),
        );
        assert!(beach_material[..2].contains(&(TerrainMaterial::Sand.index() as f32)));
        let nearby_ocean = (-1_000..=1_000).step_by(100).any(|offset_z| {
            (-1_000..=1_000).step_by(100).any(|offset_x| {
                biome_at(config, beach_x + offset_x as f32, beach_z + offset_z as f32)
                    == Biome::Ocean
            })
        });
        assert!(nearby_ocean, "beaches should border ocean regions");
    }

    #[test]
    fn same_seed_repeats_the_world_and_a_new_seed_changes_terrain() {
        let config = config();
        let changed_seed = WorldConfig {
            seed: config.seed.wrapping_add(1),
            ..config
        };
        let coord = ChunkCoord { x: -7, z: 13 };
        let first = generate_samples(config, coord);
        let repeat = generate_samples(config, coord);
        let changed = generate_samples(changed_seed, coord);

        assert_eq!(first, repeat);
        assert_ne!(first, changed);
        assert!(
            first
                .iter()
                .zip(&changed)
                .filter(|(left, right)| (left.height - right.height).abs() > 0.01)
                .count()
                > first.len() / 4,
            "a different seed should reshape the majority of a chunk"
        );
    }

    #[test]
    fn generated_chunks_have_mountains_rivers_and_valid_mesh_ranges() {
        let config = config();
        let river_z: i32 = (-120..=120)
            .find(|z| crate::river::is_river(config, 16.0, *z as f32, 0.0))
            .unwrap();
        let chunk = generate_chunk(
            config,
            ChunkCoord {
                x: 0,
                z: river_z.div_euclid(config.chunk_size_m as i32),
            },
        );
        let heights = chunk
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .collect::<Vec<_>>();

        assert!(heights.iter().all(|height| height.is_finite()));
        assert!(chunk.vertices.iter().all(|vertex| {
            vertex.color[..2]
                .iter()
                .all(|id| (0.0..=5.0).contains(id) && *id == id.round())
                && (0.0..=1.0).contains(&vertex.color[2])
        }));
        let water = crate::river::generate_chunk(
            config,
            ChunkCoord {
                x: 0,
                z: river_z.div_euclid(config.chunk_size_m as i32),
            },
        );
        assert!(!water.indices.is_empty());
        assert!(chunk
            .chunks
            .iter()
            .all(|draw| draw.indices.end <= chunk.indices.len()));
    }
}
