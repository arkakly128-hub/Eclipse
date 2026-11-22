use glam::Vec2;

pub const OCEAN_STYLE: f32 = 0.0;
pub const RIVER_STYLE: f32 = 2.0;
pub const MAX_WATER_WAVE_HEIGHT: f32 = 0.61;

pub fn depth_factor(depth: f32, style: f32) -> f32 {
    let full_depth = if style == OCEAN_STYLE {
        14.0
    } else if style == RIVER_STYLE {
        2.5
    } else {
        return depth.clamp(0.0, 1.0);
    };

    ((depth - MAX_WATER_WAVE_HEIGHT) / (full_depth - MAX_WATER_WAVE_HEIGHT)).clamp(0.0, 1.0)
}

pub fn clip_triangle(triangle: [[f32; 4]; 3], minimum_depth: f32) -> ([[f32; 4]; 4], usize) {
    let mut clipped = [[0.0; 4]; 4];
    let mut count = 0;
    let mut previous = triangle[2];
    let mut previous_inside = previous[3] >= minimum_depth;

    for current in triangle {
        let current_inside = current[3] >= minimum_depth;
        if current_inside != previous_inside {
            let blend =
                ((minimum_depth - previous[3]) / (current[3] - previous[3])).clamp(0.0, 1.0);
            clipped[count] = std::array::from_fn(|index| {
                previous[index] + (current[index] - previous[index]) * blend
            });
            count += 1;
        }
        if current_inside {
            clipped[count] = current;
            count += 1;
        }
        previous = current;
        previous_inside = current_inside;
    }

    (clipped, count)
}

pub fn height(x: f32, z: f32, time: f32, _style: f32) -> f32 {
    let position = Vec2::new(x, z);
    wave(position, Vec2::new(1.0, 0.25), 0.30, 0.42, 0.85, time)
        + wave(position, Vec2::new(0.25, 1.0), 0.15, 0.78, 1.20, time)
        + wave(position, Vec2::new(0.8, -0.65), 0.08, 1.35, 1.55, time)
        + wave(position, Vec2::new(-0.4, 1.0), 0.045, 2.4, 2.1, time)
        + wave(position, Vec2::new(0.65, -0.75), 0.035, 2.8, 2.6, time)
}

fn wave(
    position: Vec2,
    direction: Vec2,
    amplitude: f32,
    frequency: f32,
    speed: f32,
    time: f32,
) -> f32 {
    let direction = direction.normalize();
    amplitude * (position.dot(direction) * frequency + time * speed).sin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_surface_moves_over_time() {
        let still = height(13.0, -7.0, 0.0, OCEAN_STYLE);
        let moving = height(13.0, -7.0, 0.7, OCEAN_STYLE);
        assert!((still - moving).abs() > 0.01);
    }

    #[test]
    fn ocean_chop_stays_within_the_shoreline_wave_clearance() {
        let maximum = (-20..=20)
            .flat_map(|z| {
                (-20..=20).flat_map(move |x| {
                    [0.0, 0.2, 0.6, 1.0]
                        .map(move |time| height(x as f32, z as f32, time, OCEAN_STYLE).abs())
                })
            })
            .fold(0.0_f32, f32::max);

        assert!(maximum <= MAX_WATER_WAVE_HEIGHT + 0.0001);
        assert!(maximum > 0.4, "ocean waves should be visibly animated");
    }

    #[test]
    fn rivers_and_oceans_share_the_same_wave_profile_and_clearance() {
        for style in [OCEAN_STYLE, RIVER_STYLE] {
            assert_eq!(depth_factor(MAX_WATER_WAVE_HEIGHT, style), 0.0);
        }
        assert_eq!(
            height(13.0, -7.0, 0.7, OCEAN_STYLE),
            height(13.0, -7.0, 0.7, RIVER_STYLE)
        );
    }

    #[test]
    fn shoreline_clipping_interpolates_boundary_vertices() {
        let triangle = [
            [0.0, 1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0, 2.0],
            [0.0, 1.0, 1.0, 2.0],
        ];
        let (clipped, count) = clip_triangle(triangle, 1.0);

        assert_eq!(count, 4);
        assert!(clipped[..count].iter().all(|vertex| vertex[3] >= 1.0));
        assert!(clipped[..count]
            .iter()
            .any(|vertex| (vertex[3] - 1.0).abs() < f32::EPSILON));
    }

    #[test]
    fn shoreline_depth_factor_starts_at_zero_and_reaches_full_depth() {
        assert_eq!(depth_factor(MAX_WATER_WAVE_HEIGHT, OCEAN_STYLE), 0.0);
        assert_eq!(depth_factor(14.0, OCEAN_STYLE), 1.0);
        assert_eq!(depth_factor(MAX_WATER_WAVE_HEIGHT, RIVER_STYLE), 0.0);
        assert_eq!(depth_factor(2.5, RIVER_STYLE), 1.0);
    }
}
