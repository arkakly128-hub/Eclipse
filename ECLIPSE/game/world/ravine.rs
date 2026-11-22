use crate::ocean;
use crate::terrain;
use crate::world::WorldConfig;

const RAVINE_SPACING_M: f32 = 1_200.0;

pub fn carve_terrain(config: WorldConfig, x: f32, z: f32, base_height: f32, is_ocean: bool) -> f32 {
    if is_ocean {
        return base_height;
    }

    let distance = ravine_distance(config, x, z);
    let width = ravine_width(config, z);
    let carve = 1.0 - terrain::smoothstep(width, width + 24.0, distance);
    let depth = 18.0 + config.mountain_height_m * 0.22;
    let floor = (base_height - depth)
        .max(ocean::SEA_LEVEL + 1.0)
        .min(base_height);
    base_height + (floor - base_height) * carve
}

pub fn is_ravine(config: WorldConfig, x: f32, z: f32, bank_margin: f32) -> bool {
    ravine_distance(config, x, z) <= ravine_width(config, z) + bank_margin
}

pub(crate) fn material_weight(config: WorldConfig, x: f32, z: f32) -> f32 {
    1.0 - terrain::smoothstep(
        ravine_width(config, z),
        ravine_width(config, z) + 24.0,
        ravine_distance(config, x, z),
    )
}

fn ravine_distance(config: WorldConfig, x: f32, z: f32) -> f32 {
    let band = (z / RAVINE_SPACING_M).round() as i64;
    let phase = terrain::hash_unit(band, 0, config.seed, 0x7f4a_7c15) * std::f32::consts::TAU;
    let center = band as f32 * RAVINE_SPACING_M
        + (x * 0.002 + phase).sin() * 42.0
        + (x * 0.006 + phase * 1.9).sin() * 17.0
        + (terrain::value_noise(x * 0.003, band as f32, config.seed, 0x94d0_49bb) - 0.5) * 24.0;
    (z - center).abs()
}

fn ravine_width(config: WorldConfig, z: f32) -> f32 {
    let band = (z / RAVINE_SPACING_M).round() as i64;
    3.0 + terrain::hash_unit(band, 0, config.seed, 0x1331_11eb) * 3.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ravines_carve_long_recessed_channels() {
        let config = WorldConfig {
            chunk_size_m: 32,
            streaming_radius_chunks: 2,
            seed: 414165,
            tree_spacing_m: 14,
            river_spacing_m: 320,
            mountain_height_m: 64.0,
            ..WorldConfig::default()
        };
        let z = (-240..=240)
            .find(|z| {
                is_ravine(config, 16.0, *z as f32, 0.0)
                    && !crate::river::is_river(config, 16.0, *z as f32, 12.0)
                    && !ocean::is_ocean(config, 16.0, *z as f32)
            })
            .expect("generated region should contain a dry ravine");
        assert_eq!(
            terrain::biome_at(config, 16.0, z as f32),
            terrain::Biome::Ravine
        );

        assert!(
            terrain::height_at(config, 16.0, z as f32)
                < terrain::height_at(config, 16.0, z as f32 + 40.0)
        );
    }
}
