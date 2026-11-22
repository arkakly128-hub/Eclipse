use super::layout::{fit_text_scale, UiLayout};
use super::ux::{Canvas, Rect};
use crate::inventory::{Inventory, ItemStack, STORAGE_SLOTS};
use crate::player::{PlayerVitals, MAX_AIR_SECONDS};
use crate::world::GameplayConfig;
use glam::Vec3;

pub fn draw_hotbar(
    canvas: &mut Canvas<'_>,
    width: u32,
    height: u32,
    inventory: &Inventory,
    vitals: PlayerVitals,
    gameplay: GameplayConfig,
    layout: &UiLayout,
) {
    draw_vitals(canvas, width, height, vitals, gameplay, layout);
    let selected = inventory.selected_hotbar_slot();
    let ui_scale = layout.ui_scale(width, height);

    for index in 0..8 {
        let slot = layout.scale_rect(layout.hud.hotbar[index], width, height);
        canvas.image_colored(
            slot,
            3.0,
            if index == selected {
                [1.3, 1.12, 0.82, 1.0]
            } else {
                [1.0, 1.0, 1.0, 1.0]
            },
        );
        let number = layout.scale_rect(layout.hud.hotbar_numbers[index], width, height);
        let label = ((b'1' + index as u8) as char).to_string();
        let number_scale = fit_text_scale(number, &label, 2.0, 40.0, ui_scale);
        canvas.centered_text(
            number.x + number.width * 0.5,
            number.y + (number.height - 7.0 * number_scale) * 0.5,
            &label,
            number_scale,
            [0.96, 0.94, 1.0, 1.0],
        );
        if let Some(stack) = inventory.slot(STORAGE_SLOTS + index) {
            draw_item_stack(canvas, slot, stack);
        }
    }
}

pub fn draw_coordinates(canvas: &mut Canvas<'_>, width: u32, height: u32, position: Vec3) {
    let scale = (height as f32 / 720.0).clamp(0.65, 1.25);
    let label = format!(
        "X {:>7.2}  Y {:>7.2}  Z {:>7.2}",
        position.x, position.y, position.z
    );
    let padding = 8.0 * scale;
    let text_width = label.chars().count() as f32 * 6.125 * scale;
    let panel = Rect {
        x: (width as f32 - text_width - padding * 2.0 - 18.0 * scale).max(0.0),
        y: 14.0 * scale,
        width: text_width + padding * 2.0 + 18.0 * scale,
        height: 28.0 * scale,
    };
    canvas.fill(panel, [0.015, 0.025, 0.03, 0.78]);
    canvas.outline(panel, scale, [0.55, 0.68, 0.61, 0.85]);
    canvas.text(
        panel.x + padding,
        panel.y + (panel.height - 7.0 * scale) * 0.5,
        &label,
        scale,
        [0.91, 0.96, 0.92, 1.0],
    );
}

pub(super) fn draw_item_stack(canvas: &mut Canvas<'_>, rect: Rect, stack: ItemStack) {
    let item = stack.item().0;
    let icon_size = rect.width * 0.42;
    canvas.fill(
        Rect {
            x: rect.x + (rect.width - icon_size) * 0.5,
            y: rect.y + (rect.height - icon_size) * 0.5,
            width: icon_size,
            height: icon_size,
        },
        [
            0.28 + f32::from(item & 0x3f) / 128.0,
            0.34 + f32::from((item >> 3) & 0x3f) / 128.0,
            0.40 + f32::from((item >> 6) & 0x3f) / 128.0,
            1.0,
        ],
    );
    if stack.count() > 1 {
        let scale = (rect.width / 50.0).max(0.65);
        let count = stack.count().to_string();
        let text_width = count.len() as f32 * 6.0 * scale - scale;
        canvas.text(
            rect.x + rect.width - text_width - 3.0,
            rect.y + rect.height - 8.0 * scale - 2.0,
            &count,
            scale,
            [0.015, 0.02, 0.03, 1.0],
        );
    }
}

pub(super) fn draw_vitals(
    canvas: &mut Canvas<'_>,
    width: u32,
    height: u32,
    vitals: PlayerVitals,
    gameplay: GameplayConfig,
    layout: &UiLayout,
) {
    let heart_value = gameplay.max_health_units / layout.hud.hearts.len() as f32;
    for index in 0..10 {
        let remaining = (vitals.health - index as f32 * heart_value) / heart_value;
        let mut heart = layout.scale_rect(layout.hud.hearts[index], width, height);
        heart.x += index as f32 * heart.width;
        heart.width *= 2.0;
        heart.height *= 2.0;
        canvas.heart(heart, remaining);
    }

    draw_meter(
        canvas,
        width,
        height,
        layout,
        "STAMINA",
        layout.hud.stamina_label,
        layout.hud.stamina_bar,
        vitals.stamina / gameplay.max_stamina_units,
        [0.77, 0.86, 0.38, 1.0],
    );
    draw_meter(
        canvas,
        width,
        height,
        layout,
        "FOOD",
        layout.hud.food_label,
        layout.hud.food_bar,
        vitals.food / gameplay.max_food_units,
        [0.95, 0.58, 0.25, 1.0],
    );
    if vitals.air < MAX_AIR_SECONDS {
        let mut air_label = layout.hud.food_label;
        air_label.y = layout.hud.food_bar.y + layout.hud.food_bar.height + 5.0;
        let mut air_bar = layout.hud.food_bar;
        air_bar.y = air_label.y;
        draw_meter(
            canvas,
            width,
            height,
            layout,
            "AIR",
            air_label,
            air_bar,
            vitals.air / MAX_AIR_SECONDS,
            [0.24, 0.72, 0.9, 1.0],
        );
    }
}

fn draw_meter(
    canvas: &mut Canvas<'_>,
    width: u32,
    height: u32,
    layout: &UiLayout,
    label: &str,
    label_rect: Rect,
    bar_rect: Rect,
    fraction: f32,
    fill_color: [f32; 4],
) {
    let label_rect = layout.scale_rect(label_rect, width, height);
    let bar = layout.scale_rect(bar_rect, width, height);
    let ui_scale = layout.ui_scale(width, height);
    let text_scale = fit_text_scale(label_rect, label, 1.5, 18.0, ui_scale);
    let label_x = label_rect.x + label_rect.width * 0.5;
    let label_y = bar.y + (bar.height - 7.0 * text_scale) * 0.5;
    let outline_offset = 0.8 * ui_scale;
    let outline_color = [0.01, 0.015, 0.02, 1.0];
    for [offset_x, offset_y] in [
        [-1.0, -1.0],
        [0.0, -1.0],
        [1.0, -1.0],
        [-1.0, 0.0],
        [1.0, 0.0],
        [-1.0, 1.0],
        [0.0, 1.0],
        [1.0, 1.0],
    ] {
        canvas.centered_text(
            label_x + offset_x * outline_offset,
            label_y + offset_y * outline_offset,
            label,
            text_scale,
            outline_color,
        );
    }
    canvas.centered_text(label_x, label_y, label, text_scale, [0.98, 0.98, 0.96, 1.0]);
    canvas.fill(bar, [0.035, 0.045, 0.06, 1.0]);
    canvas.outline(bar, ui_scale, [0.62, 0.67, 0.73, 1.0]);
    canvas.fill(
        Rect {
            x: bar.x + 2.0 * ui_scale,
            y: bar.y + 2.0 * ui_scale,
            width: (bar.width - 4.0 * ui_scale) * fraction.clamp(0.0, 1.0),
            height: bar.height - 4.0 * ui_scale,
        },
        fill_color,
    );
}
