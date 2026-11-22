use glam::Vec3;

pub const GRID_SIDE: usize = 32;
pub const GRID_CELLS: usize = GRID_SIDE * GRID_SIDE;

const HALF_GRID: i32 = (GRID_SIDE / 2) as i32;
const FIXED_STEP: f32 = 1.0 / 120.0;
const WAVE_SPEED: f32 = 2.8;
const DAMPING: f32 = 1.4;

pub struct WaterRippleSimulation {
    heights: [f32; GRID_CELLS],
    vertical_velocity: [f32; GRID_CELLS],
    next_heights: [f32; GRID_CELLS],
    next_velocity: [f32; GRID_CELLS],
    flow_x: [f32; GRID_CELLS],
    flow_z: [f32; GRID_CELLS],
    next_flow_x: [f32; GRID_CELLS],
    next_flow_z: [f32; GRID_CELLS],
    pressure: [f32; GRID_CELLS],
    next_pressure: [f32; GRID_CELLS],
    divergence: [f32; GRID_CELLS],
    origin_x: i32,
    origin_z: i32,
    accumulator: f32,
    splash_cooldown: f32,
    previous_position: Option<Vec3>,
    was_in_water: bool,
}

impl Default for WaterRippleSimulation {
    fn default() -> Self {
        Self {
            heights: [0.0; GRID_CELLS],
            vertical_velocity: [0.0; GRID_CELLS],
            next_heights: [0.0; GRID_CELLS],
            next_velocity: [0.0; GRID_CELLS],
            flow_x: [0.0; GRID_CELLS],
            flow_z: [0.0; GRID_CELLS],
            next_flow_x: [0.0; GRID_CELLS],
            next_flow_z: [0.0; GRID_CELLS],
            pressure: [0.0; GRID_CELLS],
            next_pressure: [0.0; GRID_CELLS],
            divergence: [0.0; GRID_CELLS],
            origin_x: 0,
            origin_z: 0,
            accumulator: 0.0,
            splash_cooldown: 0.0,
            previous_position: None,
            was_in_water: false,
        }
    }
}

impl WaterRippleSimulation {
    pub fn update(
        &mut self,
        position: Vec3,
        water_surface: Option<f32>,
        moving: bool,
        delta_seconds: f32,
    ) -> Result<(), &'static str> {
        if !position.is_finite()
            || water_surface
                .map(|height| !height.is_finite())
                .unwrap_or(false)
        {
            return Err("water ripple input contains a non-finite world position or surface");
        }
        let next_origin_x =
            (position.x.floor() - HALF_GRID as f32).clamp(i32::MIN as f32, i32::MAX as f32) as i32;
        let next_origin_z =
            (position.z.floor() - HALF_GRID as f32).clamp(i32::MIN as f32, i32::MAX as f32) as i32;
        self.recenter(next_origin_x, next_origin_z);

        let in_water = water_surface
            .map(|surface| position.y <= surface + 0.25)
            .unwrap_or(false);
        let fell_into_water = match (self.previous_position, water_surface) {
            (Some(previous), Some(surface)) => {
                previous.y > surface + 0.35 && position.y <= surface + 0.25
            }
            _ => false,
        };
        if (in_water && !self.was_in_water) || fell_into_water {
            self.splash(position.x, position.z, 7.0)?;
            self.splash_cooldown = 0.2;
        } else if in_water && moving && self.splash_cooldown <= 0.0 {
            self.splash(position.x, position.z, 2.4)?;
            self.splash_cooldown = 0.28;
        }

        self.was_in_water = in_water;
        self.previous_position = Some(position);
        self.splash_cooldown = (self.splash_cooldown - delta_seconds).max(0.0);

        self.accumulator += delta_seconds.clamp(0.0, 0.05);
        while self.accumulator >= FIXED_STEP {
            self.step();
            self.accumulator -= FIXED_STEP;
        }
        Ok(())
    }

    pub fn splash(
        &mut self,
        world_x: f32,
        world_z: f32,
        strength: f32,
    ) -> Result<(), &'static str> {
        if !world_x.is_finite() || !world_z.is_finite() || !strength.is_finite() {
            return Err("water splash contains non-finite values");
        }
        let center_x = (world_x - self.origin_x as f32).floor() as i32;
        let center_z = (world_z - self.origin_z as f32).floor() as i32;
        let radius = 4;
        for z in (center_z - radius).max(1)..=(center_z + radius).min(GRID_SIDE as i32 - 2) {
            for x in (center_x - radius).max(1)..=(center_x + radius).min(GRID_SIDE as i32 - 2) {
                let offset_x = (x - center_x) as f32;
                let offset_z = (z - center_z) as f32;
                let distance_squared = offset_x * offset_x + offset_z * offset_z;
                let falloff = (1.0 - distance_squared / (radius * radius) as f32).max(0.0);
                let index = Self::index(x as usize, z as usize);
                self.vertical_velocity[index] += strength * falloff * falloff;
                let distance = distance_squared.sqrt().max(0.5);
                self.flow_x[index] += offset_x / distance * strength * falloff * 0.6;
                self.flow_z[index] += offset_z / distance * strength * falloff * 0.6;
            }
        }
        Ok(())
    }

    pub fn heights(&self) -> &[f32; GRID_CELLS] {
        &self.heights
    }

    pub fn origin(&self) -> [f32; 2] {
        [self.origin_x as f32, self.origin_z as f32]
    }

    fn recenter(&mut self, origin_x: i32, origin_z: i32) {
        let shift_x = origin_x as i64 - self.origin_x as i64;
        let shift_z = origin_z as i64 - self.origin_z as i64;
        if shift_x == 0 && shift_z == 0 {
            return;
        }
        if shift_x.abs() >= GRID_SIDE as i64 || shift_z.abs() >= GRID_SIDE as i64 {
            self.heights.fill(0.0);
            self.vertical_velocity.fill(0.0);
            self.flow_x.fill(0.0);
            self.flow_z.fill(0.0);
        } else {
            let shift_x = shift_x as i32;
            let shift_z = shift_z as i32;
            let previous_heights = self.heights;
            let previous_velocity = self.vertical_velocity;
            let previous_flow_x = self.flow_x;
            let previous_flow_z = self.flow_z;
            self.heights.fill(0.0);
            self.vertical_velocity.fill(0.0);
            self.flow_x.fill(0.0);
            self.flow_z.fill(0.0);
            for z in 0..GRID_SIDE as i32 {
                for x in 0..GRID_SIDE as i32 {
                    let source_x = x + shift_x;
                    let source_z = z + shift_z;
                    if (0..GRID_SIDE as i32).contains(&source_x)
                        && (0..GRID_SIDE as i32).contains(&source_z)
                    {
                        let destination = Self::index(x as usize, z as usize);
                        let source = Self::index(source_x as usize, source_z as usize);
                        self.heights[destination] = previous_heights[source];
                        self.vertical_velocity[destination] = previous_velocity[source];
                        self.flow_x[destination] = previous_flow_x[source];
                        self.flow_z[destination] = previous_flow_z[source];
                    }
                }
            }
        }
        self.origin_x = origin_x;
        self.origin_z = origin_z;
    }

    fn step(&mut self) {
        self.step_incompressible_flow();
        let wave_speed_squared = WAVE_SPEED * WAVE_SPEED;
        let velocity_damping = (-DAMPING * FIXED_STEP).exp();
        for z in 0..GRID_SIDE {
            for x in 0..GRID_SIDE {
                let index = Self::index(x, z);
                if x == 0 || z == 0 || x + 1 == GRID_SIDE || z + 1 == GRID_SIDE {
                    self.next_heights[index] = 0.0;
                    self.next_velocity[index] = 0.0;
                    continue;
                }
                let back_x = x as f32 - self.flow_x[index] * FIXED_STEP;
                let back_z = z as f32 - self.flow_z[index] * FIXED_STEP;
                let local_height = Self::sample(&self.heights, back_x, back_z);
                let local_velocity = Self::sample(&self.vertical_velocity, back_x, back_z);
                let laplacian = self.heights[Self::index(x - 1, z)]
                    + self.heights[Self::index(x + 1, z)]
                    + self.heights[Self::index(x, z - 1)]
                    + self.heights[Self::index(x, z + 1)]
                    - 4.0 * self.heights[index];
                let velocity = (local_velocity + wave_speed_squared * laplacian * FIXED_STEP)
                    * velocity_damping;
                self.next_velocity[index] = velocity.clamp(-12.0, 12.0);
                self.next_heights[index] =
                    (local_height + self.next_velocity[index] * FIXED_STEP).clamp(-1.25, 1.25);
            }
        }
        std::mem::swap(&mut self.heights, &mut self.next_heights);
        std::mem::swap(&mut self.vertical_velocity, &mut self.next_velocity);
    }

    fn step_incompressible_flow(&mut self) {
        let damping = (-0.8 * FIXED_STEP).exp();
        for z in 0..GRID_SIDE {
            for x in 0..GRID_SIDE {
                let index = Self::index(x, z);
                if x == 0 || z == 0 || x + 1 == GRID_SIDE || z + 1 == GRID_SIDE {
                    self.next_flow_x[index] = 0.0;
                    self.next_flow_z[index] = 0.0;
                    continue;
                }
                let back_x = x as f32 - self.flow_x[index] * FIXED_STEP;
                let back_z = z as f32 - self.flow_z[index] * FIXED_STEP;
                self.next_flow_x[index] = Self::sample(&self.flow_x, back_x, back_z) * damping;
                self.next_flow_z[index] = Self::sample(&self.flow_z, back_x, back_z) * damping;
            }
        }
        std::mem::swap(&mut self.flow_x, &mut self.next_flow_x);
        std::mem::swap(&mut self.flow_z, &mut self.next_flow_z);

        self.pressure.fill(0.0);
        for z in 1..GRID_SIDE - 1 {
            for x in 1..GRID_SIDE - 1 {
                let index = Self::index(x, z);
                self.divergence[index] = 0.5
                    * (self.flow_x[Self::index(x + 1, z)] - self.flow_x[Self::index(x - 1, z)]
                        + self.flow_z[Self::index(x, z + 1)]
                        - self.flow_z[Self::index(x, z - 1)]);
            }
        }
        for _ in 0..8 {
            for z in 1..GRID_SIDE - 1 {
                for x in 1..GRID_SIDE - 1 {
                    let index = Self::index(x, z);
                    self.next_pressure[index] = 0.25
                        * (self.pressure[Self::index(x - 1, z)]
                            + self.pressure[Self::index(x + 1, z)]
                            + self.pressure[Self::index(x, z - 1)]
                            + self.pressure[Self::index(x, z + 1)]
                            - self.divergence[index]);
                }
            }
            std::mem::swap(&mut self.pressure, &mut self.next_pressure);
        }
        for z in 1..GRID_SIDE - 1 {
            for x in 1..GRID_SIDE - 1 {
                let index = Self::index(x, z);
                self.flow_x[index] = (self.flow_x[index]
                    - 0.5
                        * (self.pressure[Self::index(x + 1, z)]
                            - self.pressure[Self::index(x - 1, z)]))
                .clamp(-8.0, 8.0);
                self.flow_z[index] = (self.flow_z[index]
                    - 0.5
                        * (self.pressure[Self::index(x, z + 1)]
                            - self.pressure[Self::index(x, z - 1)]))
                .clamp(-8.0, 8.0);
            }
        }
    }

    fn sample(field: &[f32; GRID_CELLS], x: f32, z: f32) -> f32 {
        let x = x.clamp(0.0, GRID_SIDE as f32 - 1.001);
        let z = z.clamp(0.0, GRID_SIDE as f32 - 1.001);
        let x0 = x.floor() as usize;
        let z0 = z.floor() as usize;
        let x1 = x0 + 1;
        let z1 = z0 + 1;
        let blend_x = x - x0 as f32;
        let blend_z = z - z0 as f32;
        let low = field[Self::index(x0, z0)]
            + (field[Self::index(x1, z0)] - field[Self::index(x0, z0)]) * blend_x;
        let high = field[Self::index(x0, z1)]
            + (field[Self::index(x1, z1)] - field[Self::index(x0, z1)]) * blend_x;
        low + (high - low) * blend_z
    }

    const fn index(x: usize, z: usize) -> usize {
        z * GRID_SIDE + x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ripples_remain_finite_and_recenter_with_the_player() {
        let mut simulation = WaterRippleSimulation::default();
        simulation
            .update(Vec3::new(16.0, 0.0, 16.0), None, false, 1.0 / 60.0)
            .unwrap();
        simulation.splash(16.0, 16.0, 7.0).unwrap();
        assert!(simulation
            .flow_x
            .iter()
            .any(|velocity| velocity.abs() > 0.0));
        for _ in 0..240 {
            simulation
                .update(Vec3::new(16.0, 0.0, 16.0), None, false, 1.0 / 60.0)
                .unwrap();
        }
        assert!(simulation.heights().iter().all(|height| height.is_finite()));
        assert!(simulation
            .flow_x
            .iter()
            .chain(simulation.flow_z.iter())
            .all(|velocity| velocity.is_finite() && velocity.abs() <= 8.0));
        assert!(simulation
            .heights()
            .iter()
            .any(|height| height.abs() > 0.01));

        simulation
            .update(Vec3::new(17.0, 0.0, 16.0), None, false, 1.0 / 60.0)
            .unwrap();
        assert_eq!(simulation.origin(), [1.0, 0.0]);
        assert!(simulation
            .heights()
            .iter()
            .all(|height| height.abs() <= 1.25));
    }

    #[test]
    fn entering_and_moving_in_water_create_impulses() {
        let mut simulation = WaterRippleSimulation::default();
        let position = Vec3::new(16.0, 2.0, 16.0);
        simulation
            .update(position, Some(2.0), false, 1.0 / 60.0)
            .unwrap();
        assert!(simulation.heights().iter().any(|height| height.abs() > 0.0));

        for _ in 0..20 {
            simulation
                .update(position, Some(2.0), true, 1.0 / 60.0)
                .unwrap();
        }
        assert!(simulation.heights().iter().all(|height| height.is_finite()));
    }
}
