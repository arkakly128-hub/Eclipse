use serde_json::Value;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorPalette {
    pub tree_trunk: [f32; 3],
    pub tree_leaves: [f32; 3],
    pub ground: [f32; 3],
    pub sun: [f32; 3],
}

impl ColorPalette {
    pub fn load() -> Result<Self, Box<dyn Error>> {
        let executable = std::env::current_exe()?;
        let packaged_path = executable
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("colour.json");
        let release_parent_path = executable
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."))
            .join("colour.json");
        let source_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("colour.json");
        let path = if packaged_path.is_file() {
            packaged_path
        } else if release_parent_path.is_file() {
            release_parent_path
        } else {
            source_path
        };
        let source = fs::read_to_string(&path)?;
        Self::parse(&source).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid color palette {}: {error}", path.display()),
            )
            .into()
        })
    }

    pub(crate) fn parse(source: &str) -> Result<Self, Box<dyn Error>> {
        let document: Value = serde_json::from_str(source)?;
        let colors = document
            .get("colors")
            .and_then(Value::as_object)
            .ok_or("color palette must contain a 'colors' object")?;

        Ok(Self {
            tree_trunk: parse_color(colors.get("tree_trunk"), "tree_trunk")?,
            tree_leaves: parse_color(colors.get("tree_leaves"), "tree_leaves")?,
            ground: parse_color(colors.get("ground"), "ground")?,
            sun: parse_color(colors.get("sun"), "sun")?,
        })
    }
}

fn parse_color(value: Option<&Value>, name: &str) -> Result<[f32; 3], Box<dyn Error>> {
    let value = value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing or invalid color '{name}'"))?;
    let hex = value
        .strip_prefix('#')
        .filter(|hex| hex.len() == 6)
        .ok_or_else(|| format!("color '{name}' must use #RRGGBB format"))?;

    let mut color = [0.0; 3];
    for (channel, pair) in color.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
        let digits = std::str::from_utf8(pair)?;
        let srgb = u8::from_str_radix(digits, 16)? as f32 / 255.0;
        *channel = if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        };
    }
    Ok(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_palette_colors_and_converts_srgb_to_linear() {
        let palette = ColorPalette::parse(include_str!("../colour.json")).unwrap();
        assert!((palette.tree_trunk[0] - 0.30054).abs() < 0.00001);
        assert!((palette.tree_trunk[1] - 0.16203).abs() < 0.00001);
        assert!((palette.tree_trunk[2] - 0.06848).abs() < 0.00001);
        assert!((0.29..0.32).contains(&palette.tree_leaves[1]));
        assert!(palette.ground[1] > palette.ground[0]);
    }

    #[test]
    fn rejects_colors_outside_hex_rgb_format() {
        let invalid = r##"{"colors":{"tree_trunk":"brown"}}"##;
        assert!(ColorPalette::parse(invalid).is_err());
    }
}
