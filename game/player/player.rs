use crate::camera::Camera;
use crate::inventory::Inventory;
use crate::world::World;
use eclipse_engine::collision::{CylinderCollider, Ray};
use eclipse_engine::input::KeyboardState;
use glam::Vec3;
use std::error::Error;
use std::io;

const PLAYER_RADIUS: f32 = 0.35;
const PLAYER_STANDING_HEIGHT: f32 = 1.8;
const PLAYER_SNEAKING_HEIGHT: f32 = 1.1;
const GRAVITY: f32 = 14.0;
const JUMP_SPEED: f32 = 5.2;
const WALK_SPEED: f32 = 4.5;
const SNEAK_SPEED: f32 = 2.0;
const BOOST_SPEED: f32 = 7.5;
const STANDING_EYE_HEIGHT: f32 = 1.65;
const SNEAKING_EYE_HEIGHT: f32 = 0.92;
const SWIM_SPEED: f32 = 3.2;
const SWIM_VERTICAL_SPEED: f32 = 3.5;
const SWIM_EYE_SURFACE_CLEARANCE: f32 = 0.1;
pub const MAX_AIR_SECONDS: f32 = 12.0;
const DROWNING_DAMAGE_PER_SECOND: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerVitals {
    pub health: f32,
    pub stamina: f32,
    pub food: f32,
    pub air: f32,
}

impl Default for PlayerVitals {
    fn default() -> Self {
        let config = crate::world::GameplayConfig::default();
        Self {
            health: config.max_health_units,
            stamina: config.max_stamina_units,
            food: config.max_food_units,
            air: MAX_AIR_SECONDS,
        }
    }
}

pub struct Player {
    position: Vec3,
    vertical_velocity: f32,
    eye_height: f32,
    grounded: bool,
    jump_was_down: bool,
    fall_origin_y: f32,
    vitals: PlayerVitals,
    inventory: Inventory,
}

impl Player {
    pub fn spawn(world: &World) -> Result<Self, Box<dyn Error>> {
        let spawn_position = world.spawn_position();
        let mut player = Self {
            position: spawn_position,
            vertical_velocity: 0.0,
            eye_height: STANDING_EYE_HEIGHT,
            grounded: false,
            jump_was_down: false,
            fall_origin_y: spawn_position.y,
            vitals: PlayerVitals {
                health: world.config.gameplay.max_health_units,
                stamina: world.config.gameplay.max_stamina_units,
                food: world.config.gameplay.max_food_units,
                air: MAX_AIR_SECONDS,
            },
            inventory: Inventory::default(),
        };
        player.snap_to_ground(world)?;
        Ok(player)
    }

    #[cfg(test)]
    pub fn update(
        &mut self,
        input: KeyboardState,
        camera: &Camera,
        world: &World,
        delta_seconds: f32,
    ) -> Result<(), Box<dyn Error>> {
        self.update_with_time(input, camera, world, delta_seconds, 0.0)
    }

    pub fn update_with_time(
        &mut self,
        input: KeyboardState,
        camera: &Camera,
        world: &World,
        delta_seconds: f32,
        time_seconds: f32,
    ) -> Result<(), Box<dyn Error>> {
        if !delta_seconds.is_finite() || delta_seconds < 0.0 || !time_seconds.is_finite() {
            return Err(io::Error::other(
                "player time values must be finite and the time step non-negative",
            )
            .into());
        }
        let delta_seconds = delta_seconds.min(0.05);
        let water_surface =
            world.water_surface_at_time(self.position.x, self.position.z, time_seconds);
        let in_water = water_surface
            .map(|surface| self.position.y < surface - 0.05)
            .unwrap_or(false);
        let gameplay = world.config.gameplay;
        let food_drain_per_second = if gameplay.food_depletion_seconds > 0.0 {
            gameplay.max_food_units / gameplay.food_depletion_seconds
        } else {
            0.0
        };
        self.vitals.food = (self.vitals.food - food_drain_per_second * delta_seconds).max(0.0);
        let sneaking = input.sneak;
        let sprinting = input.speed_up && !sneaking && !in_water && self.vitals.stamina > 0.0;
        let stamina_exertion = input.speed_up && !sneaking && self.vitals.stamina > 0.0;
        let target_eye_height = if sneaking {
            SNEAKING_EYE_HEIGHT
        } else {
            STANDING_EYE_HEIGHT
        };
        self.eye_height += (target_eye_height - self.eye_height) * (delta_seconds * 12.0).min(1.0);

        let forward = camera.forward().with_y(0.0).normalize_or_zero();
        let right = forward.cross(Vec3::Y).normalize_or_zero();
        let mut movement = Vec3::ZERO;
        movement += forward * (input.forward as i8 - input.backward as i8) as f32;
        movement += right * (input.right as i8 - input.left as i8) as f32;
        if movement.length_squared() > 1.0 {
            movement = movement.normalize();
        }

        let speed = if in_water {
            SWIM_SPEED
        } else if sneaking {
            SNEAK_SPEED
        } else if sprinting {
            BOOST_SPEED
        } else {
            WALK_SPEED
        };
        let displacement = movement * speed * delta_seconds;
        let previous_x = self.position.x;
        self.position.x += displacement.x;
        if world.trees.intersects(&self.collider(sneaking)) {
            self.position.x = previous_x;
        }

        let previous_z = self.position.z;
        self.position.z += displacement.z;
        if world.trees.intersects(&self.collider(sneaking)) {
            self.position.z = previous_z;
        }

        let jump_pressed = input.jump && !self.jump_was_down;
        self.jump_was_down = input.jump;
        let mut jumped = false;
        if !in_water
            && jump_pressed
            && self.grounded
            && self.vitals.stamina >= gameplay.jump_stamina_cost_units
        {
            self.vertical_velocity = JUMP_SPEED;
            self.grounded = false;
            self.fall_origin_y = self.position.y;
            self.vitals.stamina -= gameplay.jump_stamina_cost_units;
            jumped = true;
        }
        if stamina_exertion {
            self.vitals.stamina =
                (self.vitals.stamina - gameplay.sprint_stamina_per_second * delta_seconds).max(0.0);
        } else if !jumped && self.vitals.food > 0.0 {
            let food_factor =
                0.25 + 0.75 * (self.vitals.food / gameplay.max_food_units).clamp(0.0, 1.0);
            let requested = gameplay.stamina_regen_per_second * food_factor * delta_seconds;
            let available_stamina = (gameplay.max_stamina_units - self.vitals.stamina).max(0.0);
            let affordable_stamina = if gameplay.food_per_stamina_unit > 0.0 {
                self.vitals.food / gameplay.food_per_stamina_unit
            } else {
                f32::INFINITY
            };
            let restored = requested.min(available_stamina).min(affordable_stamina);
            self.vitals.stamina += restored;
            self.vitals.food =
                (self.vitals.food - restored * gameplay.food_per_stamina_unit).max(0.0);
        }

        if self.vitals.health > 0.0 {
            if self.vitals.food > 0.0 {
                let missing_health = (gameplay.max_health_units - self.vitals.health).max(0.0);
                let restored_health =
                    (gameplay.health_regen_per_second * delta_seconds).min(missing_health);
                let regeneration_seconds = if gameplay.health_regen_per_second > 0.0 {
                    restored_health / gameplay.health_regen_per_second
                } else {
                    0.0
                };
                let requested_food = gameplay.health_regen_food_per_second * regeneration_seconds;
                let food_factor = if requested_food > 0.0 {
                    (self.vitals.food / requested_food).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                self.vitals.health += restored_health * food_factor;
                self.vitals.food = (self.vitals.food - requested_food * food_factor).max(0.0);
            } else {
                self.vitals.health = (self.vitals.health
                    - gameplay.starvation_damage_per_second * delta_seconds)
                    .max(0.0);
            }
        }

        if in_water {
            if let Some(surface) =
                world.water_surface_at_time(self.position.x, self.position.z, time_seconds)
            {
                if input.jump && !input.speed_up {
                    self.vertical_velocity = SWIM_VERTICAL_SPEED;
                } else if input.speed_up && !input.jump {
                    self.vertical_velocity = -SWIM_VERTICAL_SPEED;
                } else {
                    let target_y = surface - self.eye_height + SWIM_EYE_SURFACE_CLEARANCE;
                    self.vertical_velocity += (target_y - self.position.y) * 2.8 * delta_seconds;
                    self.vertical_velocity *= (1.0 - 2.4 * delta_seconds).max(0.0);
                }
                self.position.y += self.vertical_velocity * delta_seconds;
                let highest_swim_position = if input.speed_up && !input.jump {
                    surface - self.eye_height - SWIM_EYE_SURFACE_CLEARANCE
                } else {
                    surface - self.eye_height + SWIM_EYE_SURFACE_CLEARANCE
                };
                if self.position.y > highest_swim_position {
                    self.position.y = highest_swim_position;
                    self.vertical_velocity = self.vertical_velocity.min(0.0);
                }
                self.grounded = false;
            }
        } else if !self.grounded || self.vertical_velocity > 0.0 {
            self.vertical_velocity -= GRAVITY * delta_seconds;
            self.position.y += self.vertical_velocity * delta_seconds;
        }
        self.follow_ground(world, !in_water)?;
        self.update_breath(self.is_underwater(world, time_seconds), delta_seconds);
        Ok(())
    }

    pub fn eye_position(&self) -> Vec3 {
        self.position + Vec3::Y * self.eye_height
    }

    pub fn position(&self) -> Vec3 {
        self.position
    }

    pub fn vitals(&self) -> PlayerVitals {
        self.vitals
    }

    pub fn is_underwater(&self, world: &World, time_seconds: f32) -> bool {
        world
            .water_surface_at_time(self.position.x, self.position.z, time_seconds)
            .map(|surface| self.eye_position().y < surface - 0.05)
            .unwrap_or(false)
    }

    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    pub fn inventory_mut(&mut self) -> &mut Inventory {
        &mut self.inventory
    }

    pub fn is_alive(&self) -> bool {
        self.vitals.health > 0.0
    }

    fn collider(&self, sneaking: bool) -> CylinderCollider {
        CylinderCollider {
            base: [self.position.x, self.position.y, self.position.z],
            radius: PLAYER_RADIUS,
            height: if sneaking {
                PLAYER_SNEAKING_HEIGHT
            } else {
                PLAYER_STANDING_HEIGHT
            },
        }
    }

    fn snap_to_ground(&mut self, world: &World) -> Result<(), Box<dyn Error>> {
        let hit = self
            .ground_hit(world)
            .ok_or_else(|| io::Error::other("player ground probe missed the world terrain"))?;
        self.position.y = hit.position[1];
        self.grounded = true;
        Ok(())
    }

    fn update_breath(&mut self, underwater: bool, delta_seconds: f32) {
        if underwater {
            if self.vitals.air > 0.0 {
                self.vitals.air = (self.vitals.air - delta_seconds).max(0.0);
            } else {
                self.vitals.health =
                    (self.vitals.health - DROWNING_DAMAGE_PER_SECOND * delta_seconds).max(0.0);
            }
        } else {
            self.vitals.air = (self.vitals.air + delta_seconds * 2.0).min(MAX_AIR_SECONDS);
        }
    }

    fn follow_ground(
        &mut self,
        world: &World,
        apply_fall_damage: bool,
    ) -> Result<(), Box<dyn Error>> {
        let hit = self
            .ground_hit(world)
            .ok_or_else(|| io::Error::other("player ground probe missed the world terrain"))?;
        if self.vertical_velocity <= 0.0 && self.position.y <= hit.position[1] + 0.12 {
            if !self.grounded {
                if apply_fall_damage {
                    let fall_distance = self.fall_origin_y - hit.position[1];
                    let damage = (fall_distance - world.config.gameplay.fall_safe_distance_m)
                        .max(0.0)
                        * world.config.gameplay.fall_damage_per_meter;
                    self.vitals.health = (self.vitals.health - damage).max(0.0);
                }
            }
            self.position.y = hit.position[1];
            self.vertical_velocity = 0.0;
            self.grounded = true;
        } else {
            if self.grounded {
                self.fall_origin_y = self.position.y;
            }
            self.grounded = false;
        }
        Ok(())
    }

    fn ground_hit(&self, world: &World) -> Option<eclipse_engine::collision::RaycastHit> {
        world.raycast_ground(
            Ray {
                origin: [self.position.x, self.position.y + 512.0, self.position.z],
                direction: [0.0, -1.0, 0.0],
            },
            1024.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn swimming_moves_up_with_space_and_down_with_shift() {
        let mut world = world::load().unwrap();
        let camera = Camera::first_person();
        let (x, z) = (-120..=120)
            .find_map(|z| {
                let x = 16.0;
                let z = z as f32;
                if !crate::river::is_river(world.config, x, z, 0.0)
                    || crate::ocean::is_ocean(world.config, x, z)
                {
                    return None;
                }
                let surface = world.water_surface_at_time(x, z, 0.0)?;
                (surface - crate::terrain::height_at(world.config, x, z) > 2.5).then_some((x, z))
            })
            .expect("generated region should contain a deep water surface");
        let ground_probe = Ray {
            origin: [x, 512.0, z],
            direction: [0.0, -1.0, 0.0],
        };
        let stream_position = Vec3::new(x, 0.0, z);
        let mut terrain_loaded = false;
        for _ in 0..500 {
            world.update_streaming(stream_position).unwrap();
            if world.raycast_ground(ground_probe, 1_024.0).is_some() {
                terrain_loaded = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(terrain_loaded, "river terrain chunk should stream in");

        let surface = world
            .water_surface_at_time(x, z, 0.0)
            .expect("water vertex should resolve to a water surface");
        let mut player = Player::spawn(&world).unwrap();
        player.position = Vec3::new(x, surface - 2.0, z);
        player.grounded = false;
        let initial_y = player.position.y;

        player
            .update_with_time(
                KeyboardState {
                    jump: true,
                    ..Default::default()
                },
                &camera,
                &world,
                0.05,
                0.0,
            )
            .unwrap();
        let risen_y = player.position.y;
        assert!(risen_y > initial_y);

        for _ in 0..20 {
            player
                .update_with_time(
                    KeyboardState {
                        speed_up: true,
                        ..Default::default()
                    },
                    &camera,
                    &world,
                    0.05,
                    0.0,
                )
                .unwrap();
        }
        assert!(player.is_underwater(&world, 0.0));
        assert!(player.vitals.air < MAX_AIR_SECONDS);
        player.vitals.air = 0.0;
        let health_before_drowning = player.vitals.health;
        for _ in 0..30 {
            player
                .update_with_time(KeyboardState::default(), &camera, &world, 0.05, 0.0)
                .unwrap();
        }
        assert!(player.vitals.health < health_before_drowning);
        let submerged_air = player.vitals.air;

        for _ in 0..20 {
            player
                .update_with_time(
                    KeyboardState {
                        jump: true,
                        ..Default::default()
                    },
                    &camera,
                    &world,
                    0.05,
                    0.0,
                )
                .unwrap();
        }
        assert!(!player.is_underwater(&world, 0.0));
        assert!(player.vitals.air > submerged_air);

        player.vitals.stamina = world.config.gameplay.max_stamina_units * 0.5;
        let stamina_before_dive = player.vitals.stamina;
        let dive_start_y = player.position.y;
        player
            .update_with_time(
                KeyboardState {
                    speed_up: true,
                    ..Default::default()
                },
                &camera,
                &world,
                0.05,
                0.0,
            )
            .unwrap();
        assert!(player.position.y < dive_start_y);
        assert!(player.vitals.stamina < stamina_before_dive);
    }

    #[test]
    fn depleted_air_causes_drowning_damage_and_recovers_at_surface() {
        let mut player = Player {
            position: Vec3::ZERO,
            vertical_velocity: 0.0,
            eye_height: STANDING_EYE_HEIGHT,
            grounded: true,
            jump_was_down: false,
            fall_origin_y: 0.0,
            vitals: PlayerVitals::default(),
            inventory: Inventory::default(),
        };
        for _ in 0..MAX_AIR_SECONDS as usize {
            player.update_breath(true, 1.0);
        }
        assert_eq!(player.vitals.air, 0.0);
        assert_eq!(player.vitals.health, PlayerVitals::default().health);

        player.update_breath(true, 1.0);
        assert_eq!(
            player.vitals.health,
            PlayerVitals::default().health - DROWNING_DAMAGE_PER_SECOND
        );

        player.update_breath(false, 0.5);
        assert_eq!(player.vitals.air, 1.0);
    }

    #[test]
    fn movement_speed_modes_and_jump_use_player_inputs() {
        let world = world::load().unwrap();
        let camera = Camera::first_person();

        let mut walker = Player::spawn(&world).unwrap();
        let walk_start = walker.position;
        walker
            .update(
                KeyboardState {
                    forward: true,
                    ..Default::default()
                },
                &camera,
                &world,
                0.05,
            )
            .unwrap();
        let walk_distance = walk_start.z - walker.position.z;
        assert!(walk_distance > 0.20);

        let mut boosted = Player::spawn(&world).unwrap();
        let boost_start = boosted.position;
        boosted
            .update(
                KeyboardState {
                    forward: true,
                    speed_up: true,
                    ..Default::default()
                },
                &camera,
                &world,
                0.05,
            )
            .unwrap();
        assert!(boost_start.z - boosted.position.z > walk_distance);

        let mut sneaking = Player::spawn(&world).unwrap();
        let sneak_start = sneaking.position;
        sneaking
            .update(
                KeyboardState {
                    forward: true,
                    sneak: true,
                    ..Default::default()
                },
                &camera,
                &world,
                0.05,
            )
            .unwrap();
        assert!(sneak_start.z - sneaking.position.z < walk_distance);
        assert!(sneaking.eye_position().y < STANDING_EYE_HEIGHT + sneak_start.y);

        let mut jumper = Player::spawn(&world).unwrap();
        let ground_y = jumper.position.y;
        let jump_input = KeyboardState {
            jump: true,
            ..Default::default()
        };
        jumper.update(jump_input, &camera, &world, 0.05).unwrap();
        let first_jump_height = jumper.position.y - ground_y;
        jumper.update(jump_input, &camera, &world, 0.05).unwrap();
        let second_jump_height = jumper.position.y - ground_y;
        assert!(first_jump_height > 0.15);
        assert!(second_jump_height > first_jump_height);
        assert!(second_jump_height - first_jump_height < first_jump_height);
    }

    #[test]
    fn sprint_and_jump_consume_stamina_and_empty_stamina_blocks_them() {
        let world = world::load().unwrap();
        let camera = Camera::first_person();

        let mut sprinter = Player::spawn(&world).unwrap();
        sprinter
            .update(
                KeyboardState {
                    speed_up: true,
                    ..Default::default()
                },
                &camera,
                &world,
                0.05,
            )
            .unwrap();
        assert!(sprinter.vitals.stamina < world.config.gameplay.max_stamina_units);

        let mut jumper = Player::spawn(&world).unwrap();
        let ground_y = jumper.position.y;
        jumper
            .update(
                KeyboardState {
                    jump: true,
                    ..Default::default()
                },
                &camera,
                &world,
                0.05,
            )
            .unwrap();
        assert!(
            jumper.vitals.stamina
                <= world.config.gameplay.max_stamina_units
                    - world.config.gameplay.jump_stamina_cost_units
        );
        assert!(jumper.position.y > ground_y);

        let mut exhausted = Player::spawn(&world).unwrap();
        exhausted.vitals.stamina = world.config.gameplay.jump_stamina_cost_units - 0.1;
        let ground_y = exhausted.position.y;
        exhausted
            .update(
                KeyboardState {
                    jump: true,
                    ..Default::default()
                },
                &camera,
                &world,
                0.05,
            )
            .unwrap();
        assert!((exhausted.position.y - ground_y).abs() < 0.001);
    }

    #[test]
    fn spawn_and_vital_rates_use_world_configuration() {
        let mut world = world::load().unwrap();
        world.config.gameplay.max_health_units = 40.0;
        world.config.gameplay.max_stamina_units = 30.0;
        world.config.gameplay.max_food_units = 50.0;
        world.config.gameplay.food_depletion_seconds = 25.0;
        world.config.gameplay.stamina_regen_per_second = 0.0;
        world.config.gameplay.health_regen_per_second = 2.0;
        let camera = Camera::first_person();

        let mut player = Player::spawn(&world).unwrap();
        assert!(player.position.x.abs() <= 3.0);
        assert!(player.position.z.abs() <= 3.0);
        let ground = world
            .raycast_ground(
                Ray {
                    origin: [
                        player.position.x,
                        player.position.y + 16.0,
                        player.position.z,
                    ],
                    direction: [0.0, -1.0, 0.0],
                },
                32.0,
            )
            .expect("spawn must be supported by loaded terrain");
        assert!((player.position.y - ground.position[1]).abs() < 0.01);
        assert_eq!(player.vitals.health, 40.0);
        assert_eq!(player.vitals.stamina, 30.0);
        assert_eq!(player.vitals.food, 50.0);
        player.vitals.health = 20.0;
        player
            .update(KeyboardState::default(), &camera, &world, 0.05)
            .unwrap();

        assert!((player.vitals.food - 49.875).abs() < 0.001);
        assert!((player.vitals.health - 20.1).abs() < 0.001);
    }

    #[test]
    fn health_regeneration_consumes_configured_food_at_the_matching_rate() {
        let mut world = world::load().unwrap();
        world.config.gameplay.food_depletion_seconds = 0.0;
        world.config.gameplay.stamina_regen_per_second = 0.0;
        world.config.gameplay.food_per_stamina_unit = 0.0;
        world.config.gameplay.health_regen_per_second = 0.35;
        world.config.gameplay.health_regen_food_per_second = 0.5;
        let camera = Camera::first_person();
        let mut player = Player::spawn(&world).unwrap();

        player.vitals.health = 50.0;
        player.vitals.food = 1.0;
        player
            .update(KeyboardState::default(), &camera, &world, 0.05)
            .unwrap();
        assert!((player.vitals.health - 50.0175).abs() < 0.00001);
        assert!((player.vitals.food - 0.975).abs() < 0.00001);

        player.vitals.health = 50.0;
        player.vitals.food = 0.01;
        player
            .update(KeyboardState::default(), &camera, &world, 0.05)
            .unwrap();
        assert!((player.vitals.health - 50.007).abs() < 0.00001);
        assert_eq!(player.vitals.food, 0.0);
    }

    #[test]
    fn food_regenerates_health_and_stamina_until_starvation() {
        let world = world::load().unwrap();
        let camera = Camera::first_person();

        let mut well_fed = Player::spawn(&world).unwrap();
        let mut hungry = Player::spawn(&world).unwrap();
        well_fed.vitals.health = 50.0;
        well_fed.vitals.stamina = 50.0;
        hungry.vitals.health = 50.0;
        hungry.vitals.stamina = 50.0;
        hungry.vitals.food = 0.0;

        well_fed
            .update(KeyboardState::default(), &camera, &world, 0.05)
            .unwrap();
        hungry
            .update(KeyboardState::default(), &camera, &world, 0.05)
            .unwrap();

        let food_drain =
            world.config.gameplay.max_food_units / world.config.gameplay.food_depletion_seconds;
        assert!(well_fed.vitals.food < world.config.gameplay.max_food_units);
        assert!(well_fed.vitals.food < world.config.gameplay.max_food_units - food_drain * 0.05);
        assert!(well_fed.vitals.health > 50.0);
        let expected_starvation_damage = world.config.gameplay.starvation_damage_per_second * 0.05;
        assert!((hungry.vitals.health - (50.0 - expected_starvation_damage)).abs() < 0.001);
        assert!(well_fed.vitals.stamina > hungry.vitals.stamina);
    }

    #[test]
    fn dangerous_falls_damage_health() {
        let world = world::load().unwrap();
        let camera = Camera::first_person();
        let mut player = Player::spawn(&world).unwrap();
        player.position.y += 8.0;
        player.fall_origin_y = player.position.y;
        player.grounded = false;

        for _ in 0..100 {
            player
                .update(KeyboardState::default(), &camera, &world, 0.05)
                .unwrap();
            if player.grounded {
                break;
            }
        }

        assert!(player.grounded);
        assert!(player.vitals.health < world.config.gameplay.max_health_units);
        assert!(player.is_alive());
    }

    #[test]
    fn player_can_walk_down_steep_ravine_banks() {
        let mut world = world::load().unwrap();
        world.config.streaming_radius_chunks = 1;
        let camera = Camera::first_person();
        let step = WALK_SPEED * 0.05;
        let mut ravine_candidate = None;
        for radius in (0..=16_384).step_by(64) {
            for offset in (-radius..=radius).step_by(64) {
                for (x, z) in [
                    (offset, -radius),
                    (offset, radius),
                    (-radius, offset),
                    (radius, offset),
                ] {
                    if crate::ravine::is_ravine(world.config, x as f32, z as f32, 80.0)
                        && !crate::ocean::is_ocean(world.config, x as f32, z as f32)
                        && crate::terrain::land_sample(world.config, x as f32, z as f32).1 > 50.0
                    {
                        ravine_candidate = Some((x, z));
                        break;
                    }
                }
                if ravine_candidate.is_some() {
                    break;
                }
            }
            if ravine_candidate.is_some() {
                break;
            }
        }
        let (candidate_x, candidate_z) =
            ravine_candidate.expect("generated world should include a dry ravine");
        let start = (-80..=80)
            .flat_map(|x| (-80..=80).map(move |z| (candidate_x + x, candidate_z + z)))
            .filter(|(x, z)| {
                crate::ravine::is_ravine(world.config, *x as f32, *z as f32, 40.0)
                    && !crate::river::is_river(world.config, *x as f32, *z as f32, 12.0)
            })
            .find_map(|(x, z)| {
                let (x, z) = (x as f32, z as f32);
                let start_z = z + step;
                let target = CylinderCollider {
                    base: [x, crate::terrain::height_at(world.config, x, z), z],
                    radius: PLAYER_RADIUS,
                    height: PLAYER_STANDING_HEIGHT,
                };
                (world.is_safe_spawn_ground(x, start_z)
                    && !world.is_safe_spawn_ground(x, z)
                    && crate::terrain::height_at(world.config, x, start_z)
                        > crate::terrain::height_at(world.config, x, z) + 0.12
                    && !world.trees.intersects(&target))
                .then_some((x, start_z))
            })
            .expect("generated world should have a walkable ravine bank");
        let mut ground_loaded = false;
        for _ in 0..1_000 {
            world
                .update_streaming(Vec3::new(start.0, 0.0, start.1))
                .unwrap();
            let start_hit = world.raycast_ground(
                Ray {
                    origin: [
                        start.0,
                        crate::terrain::height_at(world.config, start.0, start.1) + 16.0,
                        start.1,
                    ],
                    direction: [0.0, -1.0, 0.0],
                },
                32.0,
            );
            let target_hit = world.raycast_ground(
                Ray {
                    origin: [
                        start.0,
                        crate::terrain::height_at(world.config, start.0, start.1 - step) + 16.0,
                        start.1 - step,
                    ],
                    direction: [0.0, -1.0, 0.0],
                },
                32.0,
            );
            if start_hit.is_some() && target_hit.is_some() {
                ground_loaded = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            ground_loaded,
            "ravine terrain should stream before movement"
        );
        let mut player = Player {
            position: Vec3::new(
                start.0,
                crate::terrain::height_at(world.config, start.0, start.1),
                start.1,
            ),
            vertical_velocity: 0.0,
            eye_height: STANDING_EYE_HEIGHT,
            grounded: true,
            jump_was_down: false,
            fall_origin_y: crate::terrain::height_at(world.config, start.0, start.1),
            vitals: PlayerVitals::default(),
            inventory: Inventory::default(),
        };

        player
            .update(
                KeyboardState {
                    forward: true,
                    ..Default::default()
                },
                &camera,
                &world,
                0.05,
            )
            .unwrap();

        assert_eq!(player.position.x, start.0);
        assert!((player.position.z - (start.1 - step)).abs() < 0.0001);
        assert!(!player.grounded);
        let height_before_fall = player.position.y;
        player
            .update(KeyboardState::default(), &camera, &world, 0.05)
            .unwrap();
        assert!(player.position.y < height_before_fall);
    }
}
