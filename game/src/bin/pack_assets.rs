use eclipse_engine::asset_pack::AssetPack;
use eclipse_engine::asset_pack::AssetPackBuilder;
use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::path::{Component, Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let game_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_directory = game_directory
        .parent()
        .ok_or_else(|| io::Error::other("game directory has no workspace parent"))?;
    let manifest = game_directory.join("packaged_assets.txt");
    let output_directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            workspace_directory
                .join("release")
                .join("Eclipse")
                .join("assets")
        });

    let categories = ["icon", "sfx", "music", "model", "texture"];
    let mut builders = categories
        .iter()
        .map(|category| (*category, AssetPackBuilder::default()))
        .collect::<HashMap<_, _>>();
    let mut asset_counts = HashMap::new();
    let mut asset_paths = HashMap::<&str, Vec<String>>::new();
    let manifest_contents = std::fs::read_to_string(&manifest)?;
    for (line_index, line) in manifest_contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (category, entry) = line.split_once('|').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{} must contain '|'", manifest.display(), line_index + 1),
            )
        })?;
        let (asset_path, source_path) = entry.split_once('=').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{} must contain '='", manifest.display(), line_index + 1),
            )
        })?;
        let category = category.trim();
        let builder = builders.get_mut(category).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{}:{} unknown asset category '{category}'",
                    manifest.display(),
                    line_index + 1
                ),
            )
        })?;
        let source_path = Path::new(source_path.trim());
        if source_path.as_os_str().is_empty()
            || source_path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{}:{} source path must stay under the game directory",
                    manifest.display(),
                    line_index + 1
                ),
            )
            .into());
        }
        let source_path = game_directory.join(source_path);
        if !source_path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{}:{} asset source is missing: {}",
                    manifest.display(),
                    line_index + 1,
                    source_path.display()
                ),
            )
            .into());
        }
        let asset_path = asset_path.trim();
        builder.add_file(asset_path, source_path)?;
        asset_paths
            .entry(category)
            .or_default()
            .push(asset_path.to_owned());
        *asset_counts.entry(category).or_insert(0_usize) += 1;
    }

    std::fs::create_dir_all(&output_directory)?;
    for category in categories {
        let output_path = output_directory.join(format!("{category}.pak"));
        builders
            .remove(category)
            .ok_or_else(|| io::Error::other("missing asset pack builder"))?
            .write_to(&output_path)?;
        let pack = AssetPack::open(&output_path)?;
        let expected_paths = asset_paths.get(category).map(Vec::as_slice).unwrap_or(&[]);
        if pack.len() != expected_paths.len()
            || expected_paths
                .iter()
                .any(|asset_path| !pack.contains(asset_path))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to verify {}", output_path.display()),
            )
            .into());
        }
        println!(
            "Packed {} {category} assets into {}",
            asset_counts.get(category).copied().unwrap_or(0),
            output_path.display()
        );
    }
    Ok(())
}
