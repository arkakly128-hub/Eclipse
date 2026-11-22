use super::ux::Rect;
use serde::Deserialize;
use std::error::Error;
use std::io;

const DEFAULT_LAYOUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../engine/ui_layout.json"
));

#[derive(Clone, Deserialize)]
pub(super) struct UiLayout {
    version: u32,
    #[serde(rename = "referenceWidth")]
    reference_width: f32,
    #[serde(rename = "referenceHeight")]
    reference_height: f32,
    pub(super) menu: MenuLayout,
    pub(super) controls: ControlsLayout,
    pub(super) hud: HudLayout,
    pub(super) inventory: InventoryLayout,
}

#[derive(Clone, Deserialize)]
pub(super) struct MenuLayout {
    pub(super) title: Rect,
    pub(super) message: Rect,
    pub(super) divider: Rect,
    pub(super) buttons: [Rect; 3],
}

#[derive(Clone, Deserialize)]
pub(super) struct ControlsLayout {
    pub(super) panel: Rect,
    pub(super) title: Rect,
    pub(super) back: Rect,
    pub(super) actions: [Rect; 10],
    pub(super) keys: [Rect; 10],
    pub(super) separators: [Rect; 10],
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HudLayout {
    pub(super) hearts: [Rect; 10],
    pub(super) stamina_label: Rect,
    pub(super) stamina_bar: Rect,
    pub(super) food_label: Rect,
    pub(super) food_bar: Rect,
    pub(super) hotbar: [Rect; 8],
    pub(super) hotbar_numbers: [Rect; 8],
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InventoryLayout {
    pub(super) panel: Rect,
    pub(super) slots: [Rect; 32],
    pub(super) slot_numbers: [Rect; 8],
    pub(super) held_item: Rect,
}

pub(super) fn fit_text_scale(
    rect: Rect,
    text: &str,
    base_scale: f32,
    base_height: f32,
    ui_scale: f32,
) -> f32 {
    let height_scale = base_scale * (rect.height / base_height) * ui_scale;
    let width_scale = rect.width / (text.chars().count().max(1) as f32 * 6.125);
    height_scale.min(width_scale).max(0.1 * ui_scale)
}

impl UiLayout {
    pub(super) fn load_default() -> Result<Self, Box<dyn Error>> {
        let layout: Self = serde_json::from_str(DEFAULT_LAYOUT)?;
        layout.validate()?;
        Ok(layout)
    }

    pub(super) fn scale_rect(&self, rect: Rect, width: u32, height: u32) -> Rect {
        let scale_x = width as f32 / self.reference_width;
        let scale_y = height as f32 / self.reference_height;
        Rect {
            x: rect.x * scale_x,
            y: rect.y * scale_y,
            width: rect.width * scale_x,
            height: rect.height * scale_y,
        }
    }

    pub(super) fn ui_scale(&self, width: u32, height: u32) -> f32 {
        let scale_x = width as f32 / self.reference_width;
        let scale_y = height as f32 / self.reference_height;
        (scale_x + scale_y) * 0.5
    }

    fn validate(&self) -> Result<(), Box<dyn Error>> {
        if self.version != 1
            || !self.reference_width.is_finite()
            || !self.reference_height.is_finite()
            || self.reference_width <= 0.0
            || self.reference_height <= 0.0
        {
            return Err(
                io::Error::other("UI layout has an unsupported version or canvas size").into(),
            );
        }

        let mut rects = Vec::new();
        rects.extend([
            self.menu.title,
            self.menu.message,
            self.menu.divider,
            self.controls.panel,
            self.controls.title,
            self.controls.back,
            self.hud.stamina_label,
            self.hud.stamina_bar,
            self.hud.food_label,
            self.hud.food_bar,
            self.inventory.panel,
            self.inventory.held_item,
        ]);
        rects.extend(self.menu.buttons);
        rects.extend(self.controls.actions);
        rects.extend(self.controls.keys);
        rects.extend(self.controls.separators);
        rects.extend(self.hud.hearts);
        rects.extend(self.hud.hotbar);
        rects.extend(self.hud.hotbar_numbers);
        rects.extend(self.inventory.slots);
        rects.extend(self.inventory.slot_numbers);

        if rects.iter().any(|rect| {
            !rect.x.is_finite()
                || !rect.y.is_finite()
                || !rect.width.is_finite()
                || !rect.height.is_finite()
                || rect.width <= 0.0
                || rect.height <= 0.0
        }) {
            return Err(io::Error::other("UI layout contains an invalid element rectangle").into());
        }
        Ok(())
    }
}
