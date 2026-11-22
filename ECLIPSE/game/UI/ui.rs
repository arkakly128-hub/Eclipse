#[path = "hud.rs"]
mod hud;
#[path = "layout.rs"]
mod layout;
#[path = "ux.rs"]
mod ux;

use crate::inventory::{Inventory, STORAGE_SLOTS};
use crate::keybindings;
use crate::player::PlayerVitals;
use crate::world::GameplayConfig;
use eclipse_engine::input::KeyboardState;
use eclipse_engine::renderer::{UiVertex, MAX_UI_VERTICES};
use glam::Vec3;
use layout::{fit_text_scale, UiLayout};
use std::error::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Screen {
    Menu,
    Controls,
    Playing,
    Inventory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiAction {
    Continue,
    Started,
    Quit,
}

pub struct Ui {
    screen: Screen,
    previous_left_down: bool,
    previous_escape: bool,
    previous_inventory: bool,
    previous_debug_overlay: bool,
    show_coordinates: bool,
    mouse_position: [f32; 2],
    menu_message: &'static str,
    vertices: Vec<UiVertex>,
    layout: UiLayout,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new().expect("the built-in UI layout must be valid")
    }
}

impl Ui {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            screen: Screen::Menu,
            previous_left_down: false,
            previous_escape: false,
            previous_inventory: false,
            previous_debug_overlay: false,
            show_coordinates: false,
            mouse_position: [0.0; 2],
            menu_message: "WORLD READY",
            vertices: Vec::with_capacity(MAX_UI_VERTICES),
            layout: UiLayout::load_default()?,
        })
    }

    pub fn update(
        &mut self,
        input: KeyboardState,
        mouse_position: [f32; 2],
        left_down: bool,
        size: (u32, u32),
        inventory: &mut Inventory,
    ) -> UiAction {
        let clicked = left_down && !self.previous_left_down;
        self.previous_left_down = left_down;
        let escape_pressed = input.escape && !self.previous_escape;
        self.previous_escape = input.escape;
        let inventory_pressed = input.inventory && !self.previous_inventory;
        self.previous_inventory = input.inventory;
        let debug_overlay_pressed = input.debug_overlay && !self.previous_debug_overlay;
        self.previous_debug_overlay = input.debug_overlay;
        if debug_overlay_pressed && self.is_world_visible() {
            self.show_coordinates = !self.show_coordinates;
        }
        self.mouse_position = mouse_position;

        match self.screen {
            Screen::Menu => {
                if escape_pressed {
                    return UiAction::Quit;
                }
                if clicked {
                    let [play, controls, quit] = self
                        .layout
                        .menu
                        .buttons
                        .map(|rect| self.layout.scale_rect(rect, size.0, size.1));
                    if play.contains(mouse_position) {
                        self.screen = Screen::Playing;
                        self.menu_message = "WORLD READY";
                        return UiAction::Started;
                    } else if controls.contains(mouse_position) {
                        self.screen = Screen::Controls;
                    } else if quit.contains(mouse_position) {
                        return UiAction::Quit;
                    }
                }
            }
            Screen::Controls => {
                let back = self
                    .layout
                    .scale_rect(self.layout.controls.back, size.0, size.1);
                if escape_pressed || (clicked && back.contains(mouse_position)) {
                    self.screen = Screen::Menu;
                }
            }
            Screen::Playing => {
                if escape_pressed {
                    self.screen = Screen::Menu;
                    self.menu_message = "WORLD READY";
                } else if inventory_pressed {
                    self.screen = Screen::Inventory;
                } else if let Some(slot) = input.slot_keys.iter().position(|pressed| *pressed) {
                    inventory.select_hotbar_slot(slot);
                }
            }
            Screen::Inventory => {
                if escape_pressed || inventory_pressed {
                    self.screen = Screen::Playing;
                } else {
                    if let Some(slot) = input.slot_keys.iter().position(|pressed| *pressed) {
                        inventory.select_hotbar_slot(slot);
                    }
                    if clicked {
                        let slots = self
                            .layout
                            .inventory
                            .slots
                            .map(|rect| self.layout.scale_rect(rect, size.0, size.1));
                        if let Some(slot) =
                            slots.iter().position(|rect| rect.contains(mouse_position))
                        {
                            inventory.click_slot(slot);
                        }
                    }
                }
            }
        }
        UiAction::Continue
    }

    pub fn is_playing(&self) -> bool {
        self.screen == Screen::Playing
    }

    pub fn is_world_visible(&self) -> bool {
        matches!(self.screen, Screen::Playing | Screen::Inventory)
    }

    pub fn show_death(&mut self) {
        self.screen = Screen::Menu;
        self.menu_message = "YOU DIED";
    }

    pub fn build_vertices(
        &mut self,
        width: u32,
        height: u32,
        vitals: PlayerVitals,
        gameplay: GameplayConfig,
        player_position: Vec3,
        inventory: &Inventory,
    ) -> &[UiVertex] {
        self.vertices.clear();
        if width == 0 || height == 0 {
            return &self.vertices;
        }

        let show_coordinates = self.show_coordinates && self.is_world_visible();
        let mut canvas = ux::Canvas::new(width, height, &mut self.vertices);
        match self.screen {
            Screen::Menu => draw_menu(
                &mut canvas,
                width,
                height,
                self.mouse_position,
                self.menu_message,
                &self.layout,
            ),
            Screen::Controls => draw_controls(
                &mut canvas,
                width,
                height,
                self.mouse_position,
                &self.layout,
            ),
            Screen::Playing => hud::draw_hotbar(
                &mut canvas,
                width,
                height,
                inventory,
                vitals,
                gameplay,
                &self.layout,
            ),
            Screen::Inventory => {
                hud::draw_vitals(&mut canvas, width, height, vitals, gameplay, &self.layout);
                draw_inventory(
                    &mut canvas,
                    width,
                    height,
                    self.mouse_position,
                    inventory,
                    &self.layout,
                );
            }
        }
        if show_coordinates {
            hud::draw_coordinates(&mut canvas, width, height, player_position);
        }
        &self.vertices
    }
}

fn draw_inventory(
    canvas: &mut ux::Canvas<'_>,
    width: u32,
    height: u32,
    mouse: [f32; 2],
    inventory: &Inventory,
    layout: &UiLayout,
) {
    let panel = layout.scale_rect(layout.inventory.panel, width, height);
    let ui_scale = layout.ui_scale(width, height);
    canvas.image(panel, 2.0);

    let selected_slot = STORAGE_SLOTS + inventory.selected_hotbar_slot();
    for (index, rect) in layout.inventory.slots.iter().copied().enumerate() {
        let rect = layout.scale_rect(rect, width, height);
        let hovered = rect.contains(mouse);
        canvas.image_colored(
            rect,
            3.0,
            if index == selected_slot {
                [1.3, 1.12, 0.82, 1.0]
            } else if hovered {
                [1.2, 1.2, 1.2, 1.0]
            } else {
                [1.0, 1.0, 1.0, 1.0]
            },
        );

        if index >= STORAGE_SLOTS {
            let number = layout.scale_rect(
                layout.inventory.slot_numbers[index - STORAGE_SLOTS],
                width,
                height,
            );
            let label = ((b'1' + (index - STORAGE_SLOTS) as u8) as char).to_string();
            let number_scale = fit_text_scale(number, &label, 1.8, 40.0, ui_scale);
            canvas.centered_text(
                number.x + number.width * 0.5,
                number.y + (number.height - 7.0 * number_scale) * 0.5,
                &label,
                number_scale,
                [0.96, 0.94, 1.0, 1.0],
            );
        }
        if let Some(stack) = inventory.slot(index) {
            hud::draw_item_stack(canvas, rect, stack);
        }
    }

    if let Some(stack) = inventory.cursor_stack() {
        let offset = layout.scale_rect(layout.inventory.held_item, width, height);
        let held_item = ux::Rect {
            x: (mouse[0] + offset.x).min((width as f32 - offset.width).max(0.0)),
            y: (mouse[1] + offset.y).min((height as f32 - offset.height).max(0.0)),
            width: offset.width,
            height: offset.height,
        };
        canvas.fill(held_item, [0.88, 0.90, 0.93, 1.0]);
        canvas.outline(held_item, ui_scale, [0.015, 0.02, 0.03, 1.0]);
        hud::draw_item_stack(canvas, held_item, stack);
    }
}

fn draw_menu(
    canvas: &mut ux::Canvas<'_>,
    width: u32,
    height: u32,
    mouse: [f32; 2],
    message: &str,
    layout: &UiLayout,
) {
    let ui_scale = layout.ui_scale(width, height);
    canvas.image(
        ux::Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        },
        0.0,
    );
    canvas.fill(
        layout.scale_rect(layout.menu.divider, width, height),
        [0.93, 0.49, 0.25, 1.0],
    );

    let title = layout.scale_rect(layout.menu.title, width, height);
    let title_scale = fit_text_scale(title, "ECLIPSE", 7.0, 72.0, ui_scale);
    canvas.centered_text(
        title.x + title.width * 0.5,
        title.y + (title.height - 7.0 * title_scale) * 0.5,
        "ECLIPSE",
        title_scale,
        [0.97, 0.98, 1.0, 1.0],
    );

    let message_rect = layout.scale_rect(layout.menu.message, width, height);
    let message_scale = fit_text_scale(message_rect, message, 2.0, 26.0, ui_scale);
    canvas.centered_text(
        message_rect.x + message_rect.width * 0.5,
        message_rect.y + (message_rect.height - 7.0 * message_scale) * 0.5,
        message,
        message_scale,
        [0.67, 0.73, 0.80, 1.0],
    );

    let [play, controls, quit] = layout
        .menu
        .buttons
        .map(|rect| layout.scale_rect(rect, width, height));
    canvas.textured_button(
        play,
        "PLAY",
        play.contains(mouse),
        fit_text_scale(play, "PLAY", 3.0, 68.0, ui_scale),
    );
    canvas.textured_button(
        controls,
        "CONTROLS",
        controls.contains(mouse),
        fit_text_scale(controls, "CONTROLS", 2.5, 68.0, ui_scale),
    );
    canvas.textured_button(
        quit,
        "QUIT",
        quit.contains(mouse),
        fit_text_scale(quit, "QUIT", 3.0, 68.0, ui_scale),
    );
}

fn draw_controls(
    canvas: &mut ux::Canvas<'_>,
    width: u32,
    height: u32,
    mouse: [f32; 2],
    layout: &UiLayout,
) {
    let ui_scale = layout.ui_scale(width, height);
    canvas.fill(
        ux::Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        },
        [0.025, 0.035, 0.055, 1.0],
    );

    let panel = layout.scale_rect(layout.controls.panel, width, height);
    let title = layout.scale_rect(layout.controls.title, width, height);
    canvas.fill(panel, [0.035, 0.045, 0.06, 1.0]);
    canvas.outline(panel, 2.0 * ui_scale, [0.82, 0.85, 0.89, 1.0]);
    let title_scale = fit_text_scale(title, "CONTROLS", 3.0, 42.0, ui_scale);
    canvas.centered_text(
        title.x + title.width * 0.5,
        title.y + (title.height - 7.0 * title_scale) * 0.5,
        "CONTROLS",
        title_scale,
        [0.97, 0.98, 1.0, 1.0],
    );

    for (index, binding) in keybindings::CONTROL_BINDINGS.iter().enumerate() {
        let action = layout.scale_rect(layout.controls.actions[index], width, height);
        let key = layout.scale_rect(layout.controls.keys[index], width, height);
        let separator = layout.scale_rect(layout.controls.separators[index], width, height);
        let action_scale = fit_text_scale(action, binding.action, 1.8, 20.0, ui_scale);
        let key_scale = fit_text_scale(key, binding.key, 1.8, 20.0, ui_scale);
        canvas.text(
            action.x,
            action.y + (action.height - 7.0 * action_scale) * 0.5,
            binding.action,
            action_scale,
            [0.88, 0.91, 0.95, 1.0],
        );
        canvas.centered_text(
            key.x + key.width * 0.5,
            key.y + (key.height - 7.0 * key_scale) * 0.5,
            binding.key,
            key_scale,
            [0.97, 0.62, 0.34, 1.0],
        );
        canvas.fill(separator, [0.18, 0.21, 0.25, 1.0]);
    }

    let back = layout.scale_rect(layout.controls.back, width, height);
    canvas.textured_button(
        back,
        "BACK",
        back.contains(mouse),
        fit_text_scale(back, "BACK", 2.5, 58.0, ui_scale),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update_ui(ui: &mut Ui, input: KeyboardState, mouse: [f32; 2], left_down: bool) -> UiAction {
        ui.update(
            input,
            mouse,
            left_down,
            (1280, 720),
            &mut Inventory::default(),
        )
    }

    fn menu_button(ui: &Ui, index: usize) -> ux::Rect {
        ui.layout.menu.buttons[index]
    }

    #[test]
    fn play_starts_game_and_escape_returns_to_menu() {
        let mut ui = Ui::default();
        let play = menu_button(&ui, 0);
        let mouse = [play.x + play.width * 0.5, play.y + play.height * 0.5];
        assert_eq!(
            update_ui(&mut ui, KeyboardState::default(), mouse, true),
            UiAction::Started
        );
        assert!(ui.is_playing());

        let escape = KeyboardState {
            escape: true,
            ..Default::default()
        };
        assert_eq!(update_ui(&mut ui, escape, mouse, false), UiAction::Continue);
        assert!(!ui.is_playing());
    }

    #[test]
    fn edited_menu_rectangles_are_scaled_for_input_hit_testing() {
        let mut ui = Ui::default();
        ui.layout.menu.buttons[0] = ux::Rect {
            x: 100.0,
            y: 80.0,
            width: 200.0,
            height: 80.0,
        };
        assert_eq!(
            ui.update(
                KeyboardState::default(),
                [100.0, 60.0],
                true,
                (640, 360),
                &mut Inventory::default(),
            ),
            UiAction::Started
        );
        assert!(ui.is_playing());
    }

    #[test]
    fn holding_escape_does_not_quit_after_returning_to_the_menu() {
        let mut ui = Ui::default();
        let play = menu_button(&ui, 0);
        update_ui(
            &mut ui,
            KeyboardState::default(),
            [play.x + 5.0, play.y + 5.0],
            true,
        );
        update_ui(&mut ui, KeyboardState::default(), [0.0, 0.0], false);
        update_ui(
            &mut ui,
            KeyboardState {
                escape: true,
                ..Default::default()
            },
            [0.0, 0.0],
            false,
        );
        assert_eq!(
            update_ui(
                &mut ui,
                KeyboardState {
                    escape: true,
                    ..Default::default()
                },
                [0.0, 0.0],
                false,
            ),
            UiAction::Continue
        );
        assert!(!ui.is_playing());
    }

    #[test]
    fn quit_button_and_menu_escape_exit() {
        let mut ui = Ui::default();
        let quit = menu_button(&ui, 2);
        let mouse = [quit.x + quit.width * 0.5, quit.y + quit.height * 0.5];
        assert_eq!(
            update_ui(&mut ui, KeyboardState::default(), mouse, true),
            UiAction::Quit
        );

        let escape = KeyboardState {
            escape: true,
            ..Default::default()
        };
        assert_eq!(
            update_ui(&mut ui, escape, [0.0, 0.0], false),
            UiAction::Quit
        );
    }

    #[test]
    fn number_keys_select_hotbar_slots() {
        let mut ui = Ui::default();
        ui.screen = Screen::Playing;
        let input = KeyboardState {
            slot_keys: [false, false, false, false, false, false, false, true],
            ..Default::default()
        };
        let mut inventory = Inventory::default();
        ui.update(input, [0.0, 0.0], false, (1280, 720), &mut inventory);
        assert_eq!(inventory.selected_hotbar_slot(), 7);
        let vertices = ui.build_vertices(
            1280,
            720,
            PlayerVitals::default(),
            GameplayConfig::default(),
            Vec3::new(1.0, 2.0, -3.0),
            &inventory,
        );
        assert!(!vertices.is_empty());
        assert_eq!(vertices.len() % 3, 0);
        assert!(vertices.iter().any(|vertex| vertex.texture_slot == 3.0));
    }

    #[test]
    fn f3_toggles_a_top_right_coordinate_overlay_once_per_press() {
        let mut ui = Ui::default();
        ui.screen = Screen::Playing;
        let pressed = KeyboardState {
            debug_overlay: true,
            ..Default::default()
        };
        ui.update(
            pressed,
            [0.0, 0.0],
            false,
            (1280, 720),
            &mut Inventory::default(),
        );
        assert!(ui.show_coordinates);

        ui.update(
            pressed,
            [0.0, 0.0],
            false,
            (1280, 720),
            &mut Inventory::default(),
        );
        assert!(ui.show_coordinates);

        let vertices = ui.build_vertices(
            1280,
            720,
            PlayerVitals::default(),
            GameplayConfig::default(),
            Vec3::new(12.5, 64.0, -3.25),
            &Inventory::default(),
        );
        assert!(vertices.iter().any(|vertex| {
            vertex.texture_slot == 4.0 && vertex.position[0] > 0.5 && vertex.position[1] > 0.85
        }));

        ui.update(
            KeyboardState::default(),
            [0.0, 0.0],
            false,
            (1280, 720),
            &mut Inventory::default(),
        );
        ui.update(
            pressed,
            [0.0, 0.0],
            false,
            (1280, 720),
            &mut Inventory::default(),
        );
        assert!(!ui.show_coordinates);
    }

    #[test]
    fn death_returns_to_menu_and_play_restarts_the_ui() {
        let mut ui = Ui::default();
        ui.screen = Screen::Playing;
        ui.show_death();
        assert!(!ui.is_playing());
        assert_eq!(ui.menu_message, "YOU DIED");

        let play = menu_button(&ui, 0);
        let mouse = [play.x + play.width * 0.5, play.y + play.height * 0.5];
        assert_eq!(
            update_ui(&mut ui, KeyboardState::default(), mouse, true),
            UiAction::Started
        );
        assert!(ui.is_playing());
        assert_eq!(ui.menu_message, "WORLD READY");
    }

    #[test]
    fn inventory_opens_over_the_world_and_escape_returns_to_play() {
        let mut ui = Ui::default();
        let play = menu_button(&ui, 0);
        let mouse = [play.x + play.width * 0.5, play.y + play.height * 0.5];
        update_ui(&mut ui, KeyboardState::default(), mouse, true);
        update_ui(&mut ui, KeyboardState::default(), mouse, false);

        update_ui(
            &mut ui,
            KeyboardState {
                inventory: true,
                ..Default::default()
            },
            [0.0, 0.0],
            false,
        );
        assert!(!ui.is_playing());
        assert!(ui.is_world_visible());

        update_ui(
            &mut ui,
            KeyboardState {
                escape: true,
                ..Default::default()
            },
            [0.0, 0.0],
            false,
        );
        assert!(ui.is_playing());
    }

    #[test]
    fn inventory_renders_items_and_mouse_click_picks_up_a_stack() {
        let mut ui = Ui::default();
        let mut inventory = Inventory::default();
        inventory.insert(crate::inventory::ItemId(3), 12);
        ui.screen = Screen::Inventory;

        let first_slot = ui.layout.inventory.slots[0];
        let mouse = [
            first_slot.x + first_slot.width * 0.5,
            first_slot.y + first_slot.height * 0.5,
        ];
        ui.update(
            KeyboardState::default(),
            mouse,
            true,
            (1280, 720),
            &mut inventory,
        );
        assert_eq!(inventory.cursor_stack().unwrap().count(), 12);
        assert_eq!(inventory.slot(0), None);

        let vertices = ui.build_vertices(
            1280,
            720,
            PlayerVitals::default(),
            GameplayConfig::default(),
            Vec3::ZERO,
            &inventory,
        );
        assert!(!vertices.is_empty());
        assert_eq!(vertices.len() % 3, 0);
        assert!(vertices.iter().any(|vertex| vertex.texture_slot == 2.0));
        assert!(vertices.iter().any(|vertex| vertex.texture_slot == 3.0));
        assert!(vertices.iter().any(|vertex| vertex.texture_slot == 4.0));
    }

    #[test]
    fn inventory_layout_rectangles_scale_with_the_viewport() {
        let ui = Ui::default();
        for rect in [
            ui.layout.inventory.panel,
            ui.layout.inventory.slots[0],
            ui.layout.inventory.slots[31],
        ] {
            let scaled = ui.layout.scale_rect(rect, 720, 405);
            assert!((scaled.x - rect.x * 0.5625).abs() < 0.001);
            assert!((scaled.y - rect.y * 0.5625).abs() < 0.001);
            assert!((scaled.width - rect.width * 0.5625).abs() < 0.001);
            assert!((scaled.height - rect.height * 0.5625).abs() < 0.001);
        }
    }

    #[test]
    fn controls_menu_shows_bindings_and_back_returns_to_main_menu() {
        let mut ui = Ui::default();
        let controls = menu_button(&ui, 1);
        let mouse = [
            controls.x + controls.width * 0.5,
            controls.y + controls.height * 0.5,
        ];
        update_ui(&mut ui, KeyboardState::default(), mouse, true);
        assert_eq!(ui.screen, Screen::Controls);
        assert!(!ui.is_world_visible());

        let vertices = ui.build_vertices(
            1280,
            720,
            PlayerVitals::default(),
            GameplayConfig::default(),
            Vec3::ZERO,
            &Inventory::default(),
        );
        assert!(!vertices.is_empty());
        assert!(vertices.len() <= MAX_UI_VERTICES);

        let back = ui.layout.controls.back;
        update_ui(&mut ui, KeyboardState::default(), [0.0, 0.0], false);
        update_ui(
            &mut ui,
            KeyboardState::default(),
            [back.x + back.width * 0.5, back.y + back.height * 0.5],
            true,
        );
        assert_eq!(ui.screen, Screen::Menu);
        assert!(!ui.is_world_visible());
        update_ui(&mut ui, KeyboardState::default(), [0.0, 0.0], false);
        let play = menu_button(&ui, 0);
        assert_eq!(
            update_ui(
                &mut ui,
                KeyboardState::default(),
                [play.x + play.width * 0.5, play.y + play.height * 0.5],
                true
            ),
            UiAction::Started
        );
        assert!(ui.is_world_visible());
    }
}
