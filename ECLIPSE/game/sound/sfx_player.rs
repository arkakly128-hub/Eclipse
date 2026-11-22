use eclipse_engine::asset_pack::AssetPack;
use eclipse_engine::music_system::MusicSystem;
use std::error::Error;
use std::io;
use std::path::{Component, Path, PathBuf};

enum SoundAssets {
    Directory(PathBuf),
    Pack(AssetPack),
}

pub struct SfxPlayer {
    assets: SoundAssets,
    volume: f32,
}

impl SfxPlayer {
    pub fn new(asset_directory: impl Into<PathBuf>) -> Self {
        Self {
            assets: SoundAssets::Directory(asset_directory.into()),
            volume: 0.8,
        }
    }

    pub fn from_game_assets() -> Result<Self, Box<dyn Error>> {
        let executable = std::env::current_exe()?;
        let executable_directory = executable.parent().unwrap_or_else(|| Path::new("."));
        let package_root = executable_directory
            .parent()
            .unwrap_or(executable_directory);
        let packaged_assets = package_root.join("assets").join("sfx.pak");
        let source_assets = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("sound")
            .join("SFX");
        let assets = if packaged_assets.is_file() {
            SoundAssets::Pack(AssetPack::open(packaged_assets)?)
        } else {
            SoundAssets::Directory(source_assets)
        };
        Ok(Self {
            assets,
            volume: 0.8,
        })
    }

    pub fn set_volume(&mut self, volume: f32) -> Result<(), io::Error> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sound effect volume must be finite and between 0.0 and 1.0",
            ));
        }
        self.volume = volume;
        Ok(())
    }

    pub fn play(
        &self,
        music_system: &mut MusicSystem,
        asset: impl AsRef<Path>,
    ) -> Result<(), Box<dyn Error>> {
        let asset = asset.as_ref();
        if !asset
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sound effect path must stay inside the asset directory",
            )
            .into());
        }

        match &self.assets {
            SoundAssets::Directory(asset_directory) => {
                music_system.play_sound_effect(asset_directory.join(asset), self.volume)
            }
            SoundAssets::Pack(asset_pack) => {
                let relative_path = asset.to_str().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "sound effect path must be valid UTF-8",
                    )
                })?;
                let asset_path = relative_path.replace('\\', "/");
                let reader = asset_pack.open_asset(&asset_path)?;
                music_system.play_sound_effect_from_reader(reader, self.volume)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SfxPlayer;
    use eclipse_engine::music_system::MusicSystem;

    #[test]
    fn sound_effect_paths_cannot_escape_the_asset_directory() {
        let mut music_system = MusicSystem::new();
        let player = SfxPlayer::new("game/sound/SFX");
        assert!(player.play(&mut music_system, "../outside.wav").is_err());
        assert!(player.play(&mut music_system, "C:\\outside.wav").is_err());
    }
}
