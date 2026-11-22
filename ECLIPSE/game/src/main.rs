#[path = "../camera/camera.rs"]
mod camera;
#[path = "../world/chunk_cache.rs"]
mod chunk_cache;
#[path = "../colour/colour.rs"]
mod colour;
#[path = "../time/time.rs"]
mod day_night;
#[path = "../inventory/inventory.rs"]
pub mod inventory;
#[path = "../settings/keybindings.rs"]
mod keybindings;
#[path = "../world/ocean.rs"]
mod ocean;
#[path = "../player/player.rs"]
mod player;
#[path = "../world/ravine.rs"]
mod ravine;
#[path = "../world/river.rs"]
mod river;
#[path = "../sound/mod.rs"]
pub mod sound;
#[path = "../world/terrain.rs"]
mod terrain;
#[path = "../tree/tree.rs"]
mod tree;
#[path = "../ui/ui.rs"]
mod ui;
#[path = "../world/water_ripple.rs"]
mod water_ripple;
#[path = "../world/water_waves.rs"]
mod water_waves;
#[path = "../world/world.rs"]
mod world;

use eclipse_engine::input::KeyboardState;
use eclipse_engine::music_system::MusicSystem;
use eclipse_engine::renderer::D3D11Renderer;
use eclipse_engine::window::Window;
use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let colors = colour::ColorPalette::load()?;
    let mut world = world::load()?;
    let mut window = Window::new(1280, 720)?;
    let mut player = player::Player::spawn(&world)?;
    let mut camera = camera::Camera::first_person();
    let mut water_ripples = water_ripple::WaterRippleSimulation::default();
    let mut day_night = day_night::TimeOfDay::new(
        world.config.gameplay.start_hour,
        world.config.gameplay.day_length_seconds,
    )?;
    let mut ui = ui::Ui::new()?;
    let mut music_system = MusicSystem::new();
    let _sfx_player = sound::SfxPlayer::from_game_assets()?;
    let mut renderer = D3D11Renderer::new(
        &window,
        &world.terrain,
        &world.water,
        &world.tree_mesh,
        &world.trees.instances,
        world.config.chunk_size_m as f32,
    )?;

    let start_time = Instant::now();
    let mut previous_frame = start_time;
    while window.pump_events() {
        music_system.update();
        let frame_start = Instant::now();
        let delta_seconds = (frame_start - previous_frame).as_secs_f32().min(0.05);
        previous_frame = frame_start;
        let focused = window.is_focused();
        let input = KeyboardState::poll_with_bindings(focused, keybindings::KEY_BINDINGS);
        let client_size = window.client_size();
        let ui_action = ui.update(
            input,
            window.cursor_position_client()?,
            focused && window.left_mouse_down(),
            client_size,
            player.inventory_mut(),
        );
        if ui_action == ui::UiAction::Quit {
            break;
        }
        if ui_action == ui::UiAction::Started && !player.is_alive() {
            player = player::Player::spawn(&world)?;
            water_ripples = water_ripple::WaterRippleSimulation::default();
        }
        let was_playing = ui.is_playing();
        let time_seconds = start_time.elapsed().as_secs_f32();
        window.set_mouse_capture(was_playing && focused)?;

        if was_playing {
            day_night.advance(delta_seconds)?;
            camera.update(window.mouse_delta()?);
            player.update_with_time(input, &camera, &world, delta_seconds, time_seconds)?;
            if player.is_alive() {
                let player_position = player.position();
                water_ripples.update(
                    player_position,
                    world.water_surface_at_time(player_position.x, player_position.z, time_seconds),
                    input.forward || input.backward || input.left || input.right,
                    delta_seconds,
                )?;
                if world.update_streaming(player.position())? {
                    renderer.update_world_geometry(
                        &world.terrain,
                        &world.water,
                        &world.trees.instances,
                        world.config.chunk_size_m as f32,
                    )?;
                }
            } else {
                ui.show_death();
            }
        }

        let (width, height) = window.client_size();
        if width == 0 || height == 0 {
            thread::sleep(Duration::from_millis(16));
            continue;
        }
        let world_visible = ui.is_world_visible();
        let ui_vertices = ui.build_vertices(
            width,
            height,
            player.vitals(),
            world.config.gameplay,
            player.position(),
            player.inventory(),
        );
        if world_visible {
            let eye_position = player.eye_position();
            let view_projection =
                camera.view_projection(eye_position, width as f32 / height as f32);
            renderer.render(
                &window,
                view_projection,
                eye_position,
                camera.forward(),
                time_seconds,
                player.is_underwater(&world, time_seconds),
                day_night.lighting(colors.sun),
                water_ripples.origin(),
                water_ripples.heights(),
                ui_vertices,
            )?;
        } else {
            renderer.render_menu(&window, ui_vertices)?;
        }
    }

    Ok(())
}
