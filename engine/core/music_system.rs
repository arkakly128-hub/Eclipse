use crate::asset_pack::AssetPack;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::error::Error;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::Path;

struct AudioOutput {
    _stream: OutputStream,
    handle: OutputStreamHandle,
}

pub struct MusicSystem {
    output: Option<AudioOutput>,
    music: Option<Sink>,
    sound_effects: Vec<Sink>,
    music_volume: f32,
}

impl MusicSystem {
    pub fn new() -> Self {
        Self {
            output: None,
            music: None,
            sound_effects: Vec::new(),
            music_volume: 1.0,
        }
    }

    pub fn play_music(&mut self, path: impl AsRef<Path>) -> Result<(), Box<dyn Error>> {
        self.play_music_from_reader(File::open(path)?)
    }

    pub fn play_music_from_reader<R>(&mut self, reader: R) -> Result<(), Box<dyn Error>>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        let decoder = Decoder::new(BufReader::new(reader))?;
        let sink = Sink::try_new(self.output_handle()?)?;
        sink.set_volume(self.music_volume);
        sink.append(decoder);

        if let Some(music) = self.music.replace(sink) {
            music.stop();
        }
        Ok(())
    }

    pub fn play_music_from_pack(
        &mut self,
        asset_pack: &AssetPack,
        asset_path: &str,
    ) -> Result<(), Box<dyn Error>> {
        self.play_music_from_reader(asset_pack.open_asset(asset_path)?)
    }

    pub fn pause_music(&self) {
        if let Some(music) = &self.music {
            music.pause();
        }
    }

    pub fn resume_music(&self) {
        if let Some(music) = &self.music {
            music.play();
        }
    }

    pub fn stop_music(&mut self) {
        if let Some(music) = self.music.take() {
            music.stop();
        }
    }

    pub fn set_music_volume(&mut self, volume: f32) -> Result<(), io::Error> {
        validate_volume(volume)?;
        self.music_volume = volume;
        if let Some(music) = &self.music {
            music.set_volume(volume);
        }
        Ok(())
    }

    pub fn play_sound_effect(
        &mut self,
        path: impl AsRef<Path>,
        volume: f32,
    ) -> Result<(), Box<dyn Error>> {
        self.play_sound_effect_from_reader(File::open(path)?, volume)
    }

    pub fn play_sound_effect_from_reader<R>(
        &mut self,
        reader: R,
        volume: f32,
    ) -> Result<(), Box<dyn Error>>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        validate_volume(volume)?;
        let decoder = Decoder::new(BufReader::new(reader))?;
        let sink = Sink::try_new(self.output_handle()?)?;
        sink.set_volume(volume);
        sink.append(decoder);
        self.sound_effects.push(sink);
        Ok(())
    }

    pub fn update(&mut self) {
        self.sound_effects.retain(|effect| !effect.empty());
    }

    fn output_handle(&mut self) -> Result<&OutputStreamHandle, Box<dyn Error>> {
        if self.output.is_none() {
            let (stream, handle) = OutputStream::try_default()?;
            self.output = Some(AudioOutput {
                _stream: stream,
                handle,
            });
        }

        self.output
            .as_ref()
            .map(|output| &output.handle)
            .ok_or_else(|| io::Error::other("audio output initialization failed").into())
    }
}

impl Default for MusicSystem {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_volume(volume: f32) -> Result<(), io::Error> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audio volume must be finite and between 0.0 and 1.0",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MusicSystem;

    #[test]
    fn music_volume_must_be_finite_and_in_range() {
        let mut music = MusicSystem::new();
        assert!(music.set_music_volume(0.5).is_ok());
        assert!(music.set_music_volume(-0.1).is_err());
        assert!(music.set_music_volume(1.1).is_err());
        assert!(music.set_music_volume(f32::NAN).is_err());
    }
}
