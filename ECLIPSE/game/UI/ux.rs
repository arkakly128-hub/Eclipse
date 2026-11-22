use eclipse_engine::renderer::UiVertex;
use serde::Deserialize;

const FONT_ATLAS_COLUMNS: usize = 16;
const FONT_ATLAS_ROWS: usize = 6;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(self, point: [f32; 2]) -> bool {
        point[0] >= self.x
            && point[0] <= self.x + self.width
            && point[1] >= self.y
            && point[1] <= self.y + self.height
    }
}

pub struct Canvas<'a> {
    width: f32,
    height: f32,
    vertices: &'a mut Vec<UiVertex>,
}

impl<'a> Canvas<'a> {
    pub fn new(width: u32, height: u32, vertices: &'a mut Vec<UiVertex>) -> Self {
        Self {
            width: width as f32,
            height: height as f32,
            vertices,
        }
    }

    pub fn fill(&mut self, rect: Rect, color: [f32; 4]) {
        if rect.width <= 0.0 || rect.height <= 0.0 || self.width <= 0.0 || self.height <= 0.0 {
            return;
        }
        let left = rect.x / self.width * 2.0 - 1.0;
        let right = (rect.x + rect.width) / self.width * 2.0 - 1.0;
        let top = 1.0 - rect.y / self.height * 2.0;
        let bottom = 1.0 - (rect.y + rect.height) / self.height * 2.0;
        let vertices = [
            [left, top],
            [right, top],
            [right, bottom],
            [left, top],
            [right, bottom],
            [left, bottom],
        ];
        self.vertices
            .extend(vertices.into_iter().map(|position| UiVertex {
                position,
                texcoord: [0.0, 0.0],
                color,
                texture_slot: -1.0,
            }));
    }

    pub fn image(&mut self, rect: Rect, texture_slot: f32) {
        self.image_colored(rect, texture_slot, [1.0, 1.0, 1.0, 1.0]);
    }

    pub fn image_colored(&mut self, rect: Rect, texture_slot: f32, color: [f32; 4]) {
        self.image_region(rect, texture_slot, color, [0.0, 0.0, 1.0, 1.0]);
    }

    fn image_region(&mut self, rect: Rect, texture_slot: f32, color: [f32; 4], uv: [f32; 4]) {
        if rect.width <= 0.0 || rect.height <= 0.0 || self.width <= 0.0 || self.height <= 0.0 {
            return;
        }
        let left = rect.x / self.width * 2.0 - 1.0;
        let right = (rect.x + rect.width) / self.width * 2.0 - 1.0;
        let top = 1.0 - rect.y / self.height * 2.0;
        let bottom = 1.0 - (rect.y + rect.height) / self.height * 2.0;
        let vertices = [
            ([left, top], [uv[0], uv[1]]),
            ([right, top], [uv[2], uv[1]]),
            ([right, bottom], [uv[2], uv[3]]),
            ([left, top], [uv[0], uv[1]]),
            ([right, bottom], [uv[2], uv[3]]),
            ([left, bottom], [uv[0], uv[3]]),
        ];
        self.vertices
            .extend(vertices.into_iter().map(|(position, texcoord)| UiVertex {
                position,
                texcoord,
                color,
                texture_slot,
            }));
    }

    pub fn outline(&mut self, rect: Rect, thickness: f32, color: [f32; 4]) {
        self.fill(
            Rect {
                height: thickness,
                ..rect
            },
            color,
        );
        self.fill(
            Rect {
                y: rect.y + rect.height - thickness,
                height: thickness,
                ..rect
            },
            color,
        );
        self.fill(
            Rect {
                y: rect.y + thickness,
                width: thickness,
                height: rect.height - thickness * 2.0,
                ..rect
            },
            color,
        );
        self.fill(
            Rect {
                x: rect.x + rect.width - thickness,
                y: rect.y + thickness,
                width: thickness,
                height: rect.height - thickness * 2.0,
                ..rect
            },
            color,
        );
    }

    pub fn textured_button(&mut self, rect: Rect, label: &str, hovered: bool, scale: f32) {
        self.image_colored(rect, 1.0, [1.0, 1.0, 1.0, if hovered { 1.0 } else { 0.0 }]);
        self.centered_text(
            rect.x + rect.width * 0.5,
            rect.y + (rect.height - 7.0 * scale) * 0.5,
            label,
            scale,
            [1.0, 1.0, 1.0, 1.0],
        );
    }

    pub fn centered_text(
        &mut self,
        center_x: f32,
        y: f32,
        text: &str,
        scale: f32,
        color: [f32; 4],
    ) {
        let text_width = text.chars().count() as f32 * 6.125 * scale;
        self.text(center_x - text_width * 0.5, y, text, scale, color);
    }

    pub fn text(&mut self, x: f32, y: f32, text: &str, scale: f32, color: [f32; 4]) {
        let mut cursor_x = x;
        for character in text.chars() {
            let code = u32::from(character);
            let code = if (32..128).contains(&code) {
                code
            } else {
                u32::from('?')
            };
            let atlas_index = (code - 32) as usize;
            let column = (atlas_index % FONT_ATLAS_COLUMNS) as f32;
            let row = (atlas_index / FONT_ATLAS_COLUMNS) as f32;
            let u0 = column / FONT_ATLAS_COLUMNS as f32;
            let v0 = row / FONT_ATLAS_ROWS as f32;
            let u1 = (column + 1.0) / FONT_ATLAS_COLUMNS as f32;
            let v1 = (row + 1.0) / FONT_ATLAS_ROWS as f32;
            let font_scale = scale * (7.0 / 48.0);
            self.image_region(
                Rect {
                    x: cursor_x - 1.6 * scale,
                    y: y - scale,
                    width: 9.333 * scale,
                    height: 9.333 * scale,
                },
                4.0,
                color,
                [u0, v0, u1, v1],
            );
            cursor_x += 42.0 * font_scale;
        }
    }

    pub fn heart(&mut self, rect: Rect, fraction: f32) {
        let fraction = fraction.clamp(0.0, 1.0);
        self.image(rect, 6.0);
        if fraction > 0.0 {
            self.image_region(
                Rect {
                    y: rect.y + rect.height * (1.0 - fraction),
                    height: rect.height * fraction,
                    ..rect
                },
                5.0,
                [1.0; 4],
                [0.0, 1.0 - fraction, 1.0, 1.0],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Canvas, Rect};

    #[test]
    fn text_uses_the_antialiased_font_atlas() {
        let mut vertices = Vec::new();
        {
            let mut canvas = Canvas::new(320, 100, &mut vertices);
            canvas.centered_text(160.0, 20.0, "JUMP", 2.0, [1.0; 4]);
        }
        assert_eq!(vertices.len(), 4 * 6);
        assert!(vertices.iter().all(|vertex| vertex.texture_slot == 4.0));
        assert!(vertices.iter().any(|vertex| vertex.texcoord[0] > 0.0));

        vertices.clear();
        {
            let mut canvas = Canvas::new(320, 100, &mut vertices);
            canvas.text(0.0, 0.0, "A", 1.0, [1.0; 4]);
        }
        assert!((vertices[0].texcoord[1] - (2.0 / 6.0)).abs() < f32::EPSILON);

        vertices.clear();
        {
            let mut canvas = Canvas::new(320, 100, &mut vertices);
            canvas.centered_text(160.0, 20.0, "1", 2.0, [1.0; 4]);
        }
        let left = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min);
        let right = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(((left + right) * 0.5).abs() < 0.0001);
    }

    #[test]
    fn hearts_use_black_empty_and_red_fractional_textures() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 30.0,
        };
        let mut vertices = Vec::new();
        let mut canvas = Canvas::new(1280, 720, &mut vertices);

        canvas.heart(rect, 0.0);
        assert_eq!(canvas.vertices.len(), 6);
        assert!(canvas
            .vertices
            .iter()
            .all(|vertex| vertex.texture_slot == 6.0));

        canvas.vertices.clear();
        canvas.heart(rect, 0.5);
        assert_eq!(canvas.vertices.len(), 12);
        assert!(canvas.vertices[..6]
            .iter()
            .all(|vertex| vertex.texture_slot == 6.0));
        assert!(canvas.vertices[6..]
            .iter()
            .all(|vertex| vertex.texture_slot == 5.0));
    }
}
