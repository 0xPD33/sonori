use std::f32::consts::PI;

#[derive(Debug, Clone, Copy)]
pub enum SoundType {
    RecordStart,
    RecordStop,
    SessionStart,
    SessionComplete,
    SessionCancel,
}

pub struct SoundGenerator {
    sample_rate: u32,
}

impl SoundGenerator {
    pub fn new(sample_rate: u32) -> Self {
        Self { sample_rate }
    }

    pub fn generate(&self, sound_type: SoundType) -> Vec<f32> {
        match sound_type {
            SoundType::RecordStart => self.generate_tone(440.0, 0.2, 0.3),
            SoundType::RecordStop => self.generate_tone(330.0, 0.2, 0.3),
            SoundType::SessionStart => self.generate_tone(523.0, 0.15, 0.4),
            SoundType::SessionComplete => self.generate_sweep_tone(523.0, 330.0, 0.4, 0.35),
            SoundType::SessionCancel => self.generate_tone(294.0, 0.2, 0.3),
        }
    }

    fn generate_tone(&self, frequency: f32, duration: f32, amplitude: f32) -> Vec<f32> {
        let num_samples = (self.sample_rate as f32 * duration) as usize;
        let mut samples = Vec::with_capacity(num_samples);

        for i in 0..num_samples {
            let t = i as f32 / self.sample_rate as f32;
            let envelope = self.envelope(t, duration);
            let sample = (2.0 * PI * frequency * t).sin() * amplitude * envelope;
            samples.push(sample);
        }

        samples
    }

    fn generate_sweep_tone(
        &self,
        start_freq: f32,
        end_freq: f32,
        duration: f32,
        amplitude: f32,
    ) -> Vec<f32> {
        let num_samples = (self.sample_rate as f32 * duration) as usize;
        let mut samples = Vec::with_capacity(num_samples);

        for i in 0..num_samples {
            let t = i as f32 / self.sample_rate as f32;
            let progress = t / duration;
            let frequency = start_freq + (end_freq - start_freq) * progress;
            let envelope = self.envelope(t, duration);
            let sample = (2.0 * PI * frequency * t).sin() * amplitude * envelope;
            samples.push(sample);
        }

        samples
    }

    fn envelope(&self, t: f32, duration: f32) -> f32 {
        let attack = 0.01;
        let release = 0.05;

        if t < attack {
            t / attack
        } else if t > duration - release {
            (duration - t) / release
        } else {
            1.0
        }
    }
}
