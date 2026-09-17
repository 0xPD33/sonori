use crate::config::SoundConfig;
use crate::sound_generator::{SoundGenerator, SoundType};
use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use speechcore::{FeedbackEvent, FeedbackSink};
use std::collections::VecDeque;
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
            if let Err(e) = Self::run(sound_rx) {
                eprintln!("Sound playback unavailable: {}", e);
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

    // ponytail: one stream stays open for the process lifetime and outputs silence when
    // idle. Per-cue streams lost the cue when the sink (Bluetooth) had more latency than
    // the stream lifetime, because closing an ALSA PCM discards unplayed audio.
    // Upgrade path: pause the stream after N seconds of silence if idle CPU matters.
    fn run(sound_rx: mpsc::Receiver<(SoundType, f32)>) -> Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no audio output device"))?;
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let generator = SoundGenerator::new(sample_rate);

        // Mono samples pending playback; the callback writes each one to every channel.
        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
        let queue_cb = queue.clone();

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // try_lock: never block the audio thread; a missed period plays silence.
                let mut pending = queue_cb.try_lock();
                for frame in data.chunks_mut(channels) {
                    let sample = pending
                        .as_mut()
                        .and_then(|q| q.pop_front())
                        .unwrap_or(0.0);
                    frame.fill(sample);
                }
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        )?;
        stream.play()?;

        while let Ok((sound_type, volume)) = sound_rx.recv() {
            let mut pending = queue.lock();
            pending.extend(generator.generate(sound_type).iter().map(|s| s * volume));
        }

        Ok(())
    }
}

impl FeedbackSink for SoundPlayer {
    fn play(&self, event: FeedbackEvent) {
        let sound_type = match event {
            FeedbackEvent::RecordStart => SoundType::RecordStart,
            FeedbackEvent::RecordStop => SoundType::RecordStop,
            FeedbackEvent::SessionStart => SoundType::SessionStart,
            FeedbackEvent::SessionComplete => SoundType::SessionComplete,
            FeedbackEvent::SessionCancel => SoundType::SessionCancel,
        };

        self.play(sound_type);
    }
}
