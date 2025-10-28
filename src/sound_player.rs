use crate::config::SoundConfig;
use crate::sound_generator::{SoundGenerator, SoundType};
use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

pub struct SoundPlayer {
    sound_tx: mpsc::Sender<(SoundType, f32)>,
    enabled: Arc<AtomicBool>,
    volume: Arc<Mutex<f32>>,
}

impl SoundPlayer {
    pub fn new(config: &SoundConfig) -> Result<Arc<Self>> {
        let (sound_tx, sound_rx) = mpsc::channel::<(SoundType, f32)>();
        let enabled = Arc::new(AtomicBool::new(config.enabled));
        let volume = Arc::new(Mutex::new(config.volume));

        // Use a dedicated blocking thread for sound playback (CPAL streams are not Send)
        std::thread::spawn(move || {
            let host = cpal::default_host();

            let device = match host.default_output_device() {
                Some(d) => d,
                None => {
                    eprintln!("No audio output device available for sound playback");
                    return;
                }
            };

            let config = match device.default_output_config() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to get default output config: {}", e);
                    return;
                }
            };

            let sample_rate = config.sample_rate().0;
            let generator = SoundGenerator::new(sample_rate);

            while let Ok((sound_type, volume)) = sound_rx.recv() {
                if let Err(e) = Self::play_sound_internal(&device, &generator, sound_type, volume) {
                    eprintln!("Failed to play sound {:?}: {}", sound_type, e);
                }
            }
        });

        Ok(Arc::new(Self {
            sound_tx,
            enabled,
            volume,
        }))
    }

    pub fn play(&self, sound_type: SoundType) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }

        let volume = *self.volume.lock();
        let _ = self.sound_tx.send((sound_type, volume));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn set_volume(&self, volume: f32) {
        *self.volume.lock() = volume.clamp(0.0, 1.0);
    }

    fn play_sound_internal(
        device: &cpal::Device,
        generator: &SoundGenerator,
        sound_type: SoundType,
        volume: f32,
    ) -> Result<()> {
        let mut samples = generator.generate(sound_type);

        for sample in samples.iter_mut() {
            *sample *= volume;
        }

        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;

        let mut sample_idx = 0;
        let samples_len = samples.len();
        let samples = Arc::new(samples);
        let samples_clone = samples.clone();

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for sample in data.iter_mut() {
                    if sample_idx < samples_len {
                        *sample = samples_clone[sample_idx];
                        sample_idx += 1;
                    } else {
                        *sample = 0.0;
                    }
                }
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        )?;

        stream.play()?;

        let duration_secs = samples_len as f32 / sample_rate as f32;
        std::thread::sleep(std::time::Duration::from_secs_f32(duration_secs + 0.1));

        drop(stream);

        Ok(())
    }
}
