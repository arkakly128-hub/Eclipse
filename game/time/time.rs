use eclipse_engine::renderer::SceneLighting;
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeOfDay {
    seconds_into_day: f64,
    day_length_seconds: f64,
}

impl TimeOfDay {
    pub fn new(start_hour: f32, day_length_seconds: f64) -> Result<Self, &'static str> {
        if !start_hour.is_finite() || !(0.0..24.0).contains(&start_hour) {
            return Err("time of day must be finite and in the range [0, 24)");
        }
        if !day_length_seconds.is_finite() || day_length_seconds <= 0.0 {
            return Err("day length must be finite and greater than zero");
        }
        Ok(Self {
            seconds_into_day: day_length_seconds * start_hour as f64 / 24.0,
            day_length_seconds,
        })
    }

    pub fn hour(&self) -> f32 {
        (self.seconds_into_day / self.day_length_seconds * 24.0) as f32
    }

    pub fn advance(&mut self, delta_seconds: f32) -> Result<(), &'static str> {
        if !delta_seconds.is_finite() || delta_seconds < 0.0 {
            return Err("time step must be finite and non-negative");
        }
        self.seconds_into_day =
            (self.seconds_into_day + delta_seconds as f64).rem_euclid(self.day_length_seconds);
        Ok(())
    }

    pub fn lighting(&self, base_sun_color: [f32; 3]) -> SceneLighting {
        let hour_angle = self.hour() / 24.0 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let sun_direction = Vec3::new(hour_angle.cos(), hour_angle.sin(), 0.0);
        let elevation = sun_direction.y;
        let daylight = smoothstep(-0.15, 0.20, elevation);
        let twilight = 1.0 - smoothstep(0.03, 0.38, elevation.abs());
        let warm_to_day = smoothstep(0.04, 0.36, elevation.abs());

        let base_sun = Vec3::from_array(base_sun_color);
        let dawn_tint = Vec3::new(1.12, 0.40, 0.14);
        let sun_color = base_sun * dawn_tint.lerp(Vec3::ONE, warm_to_day);
        let night_ambient = Vec3::new(0.018, 0.026, 0.055);
        let day_ambient = Vec3::new(0.19, 0.23, 0.22);
        let dawn_ambient = Vec3::new(0.25, 0.105, 0.07);
        let ambient_color = night_ambient
            .lerp(day_ambient, daylight)
            .lerp(dawn_ambient, twilight);
        let moon_intensity = (1.0 - daylight) * 0.36;

        SceneLighting {
            sun_direction,
            sun_color: sun_color.to_array(),
            sun_intensity: smoothstep(-0.15, 0.18, elevation) * 1.45,
            ambient_color: ambient_color.to_array(),
            moon_intensity,
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_SUN: [f32; 3] = [1.0, 0.8, 0.6];

    #[test]
    fn sun_tracks_the_day_and_reaches_zenith_at_noon() {
        let mut time = TimeOfDay::new(12.0, 900.0).unwrap();
        let noon = time.lighting(BASE_SUN);
        assert!((noon.sun_direction.y - 1.0).abs() < 1.0e-6);
        assert!((noon.sun_intensity - 1.45).abs() < 1.0e-6);

        time = TimeOfDay::new(0.0, 900.0).unwrap();
        let midnight = time.lighting(BASE_SUN);
        assert!((midnight.sun_direction.y + 1.0).abs() < 1.0e-6);
        assert_eq!(midnight.sun_intensity, 0.0);
        assert!(midnight.moon_intensity > 0.35);
        assert!(midnight.ambient_color[2] > midnight.ambient_color[0]);
    }

    #[test]
    fn dawn_and_dusk_share_warm_light_and_night_is_cooler() {
        let mut time = TimeOfDay::new(6.0, 900.0).unwrap();
        let dawn = time.lighting(BASE_SUN);
        time = TimeOfDay::new(18.0, 900.0).unwrap();
        let dusk = time.lighting(BASE_SUN);
        assert!((dawn.sun_color[1] - dusk.sun_color[1]).abs() < 1.0e-5);
        assert!(dawn.sun_color[0] > dawn.sun_color[2]);

        time = TimeOfDay::new(12.0, 900.0).unwrap();
        let noon = time.lighting(BASE_SUN);
        assert!(noon.ambient_color[1] > dawn.ambient_color[1]);
    }

    #[test]
    fn time_wraps_and_rejects_invalid_values() {
        let mut time = TimeOfDay::new(23.99, 900.0).unwrap();
        time.advance(1.0).unwrap();
        assert!(time.hour() < 0.1);
        assert!(TimeOfDay::new(24.0, 900.0).is_err());
        assert!(TimeOfDay::new(12.0, 0.0).is_err());
        assert!(time.advance(f32::NAN).is_err());
        assert!(time.advance(-1.0).is_err());
    }

    #[test]
    fn configured_day_length_controls_cycle_speed() {
        let mut time = TimeOfDay::new(0.0, 120.0).unwrap();
        time.advance(60.0).unwrap();
        assert!((time.hour() - 12.0).abs() < 1.0e-5);
    }
}
