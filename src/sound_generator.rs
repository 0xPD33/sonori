use std::collections::HashMap;
use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundType {
    RecordStart,
    RecordStop,
    SessionStart,
    SessionComplete,
    SessionCancel,
}

// Musical note frequencies (Hz)
const C5: f32 = 523.25;
const E5: f32 = 659.25;
const G5: f32 = 783.99;
const C4: f32 = 261.63;

pub struct SoundGenerator {
    sample_rate: u32,
    cache: HashMap<SoundType, Vec<f32>>,
}

impl SoundGenerator {
    pub fn new(sample_rate: u32) -> Self {
        let mut generator = Self {
            sample_rate,
            cache: HashMap::new(),
        };

        generator.cache.insert(
            SoundType::RecordStart,
            generator.generate_two_tone(C5, E5, 0.08, 0.12, 0.35),
        );
        generator.cache.insert(
            SoundType::RecordStop,
            generator.generate_two_tone(E5, C5, 0.08, 0.12, 0.35),
        );
        generator.cache.insert(
            SoundType::SessionStart,
            generator.generate_two_tone(C5, G5, 0.1, 0.15, 0.4),
        );
        generator.cache.insert(
            SoundType::SessionComplete,
            generator.generate_three_tone(G5, E5, C5, 0.12, 0.4),
        );
        generator.cache.insert(
            SoundType::SessionCancel,
            generator.generate_double_tap(C4, 0.06, 0.04, 0.3),
        );

        generator
    }

    pub fn generate(&self, sound_type: SoundType) -> &[f32] {
        self.cache
            .get(&sound_type)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Generate a two-note melodic sequence with crossfade
    fn generate_two_tone(
        &self,
        freq1: f32,
        freq2: f32,
        note1_dur: f32,
        note2_dur: f32,
        amplitude: f32,
    ) -> Vec<f32> {
        let crossfade_dur = 0.05; // 50ms crossfade
        let note1 = self.generate_rich_tone(freq1, note1_dur, amplitude);
        let note2 = self.generate_rich_tone(freq2, note2_dur, amplitude);
        self.crossfade_notes(note1, note2, crossfade_dur)
    }

    /// Generate a three-note melodic sequence (for completion sound) with crossfades
    fn generate_three_tone(
        &self,
        freq1: f32,
        freq2: f32,
        freq3: f32,
        note_dur: f32,
        amplitude: f32,
    ) -> Vec<f32> {
        let crossfade_dur = 0.05; // 50ms crossfade
        let note1 = self.generate_rich_tone(freq1, note_dur, amplitude);
        let note2 = self.generate_rich_tone(freq2, note_dur, amplitude);
        let note3 = self.generate_rich_tone(freq3, note_dur * 1.5, amplitude);

        // Crossfade note1 → note2
        let mut result = self.crossfade_notes(note1, note2, crossfade_dur);
        // Crossfade result → note3
        result = self.crossfade_notes(result, note3, crossfade_dur);
        result
    }

    /// Crossfade two audio segments smoothly
    fn crossfade_notes(
        &self,
        mut note1: Vec<f32>,
        note2: Vec<f32>,
        crossfade_dur: f32,
    ) -> Vec<f32> {
        let crossfade_samples = (self.sample_rate as f32 * crossfade_dur) as usize;
        let crossfade_samples = crossfade_samples.min(note1.len()).min(note2.len());

        if crossfade_samples == 0 {
            // No crossfade possible, just concatenate
            note1.extend(note2);
            return note1;
        }

        // Calculate where crossfade starts in note1
        let note1_crossfade_start = note1.len().saturating_sub(crossfade_samples);

        // Mix the overlapping region
        for i in 0..crossfade_samples {
            let fade_progress = i as f32 / crossfade_samples as f32;
            let fade_out = 1.0 - fade_progress; // Linear fade out
            let fade_in = fade_progress; // Linear fade in

            let note1_idx = note1_crossfade_start + i;
            if note1_idx < note1.len() && i < note2.len() {
                note1[note1_idx] = note1[note1_idx] * fade_out + note2[i] * fade_in;
            }
        }

        // Append the rest of note2 after the crossfade region
        if crossfade_samples < note2.len() {
            note1.extend_from_slice(&note2[crossfade_samples..]);
        }

        note1
    }

    /// Generate a quick double-tap sound (for cancel)
    fn generate_double_tap(
        &self,
        freq: f32,
        tap_dur: f32,
        gap_dur: f32,
        amplitude: f32,
    ) -> Vec<f32> {
        let mut samples = self.generate_rich_tone(freq, tap_dur, amplitude);
        // Add silence gap
        let gap_samples = (self.sample_rate as f32 * gap_dur) as usize;
        samples.extend(vec![0.0; gap_samples]);
        samples.extend(self.generate_rich_tone(freq, tap_dur, amplitude));
        samples
    }

    /// Generate a tone with harmonics for richer sound
    fn generate_rich_tone(&self, frequency: f32, duration: f32, amplitude: f32) -> Vec<f32> {
        let num_samples = (self.sample_rate as f32 * duration) as usize;
        let mut samples = Vec::with_capacity(num_samples);

        for i in 0..num_samples {
            let t = i as f32 / self.sample_rate as f32;
            let envelope = self.smooth_envelope(t, duration);

            // Fundamental + harmonics for warmer sound
            let fundamental = (2.0 * PI * frequency * t).sin();
            let harmonic_2 = (2.0 * PI * frequency * 2.0 * t).sin() * 0.3;
            let harmonic_3 = (2.0 * PI * frequency * 3.0 * t).sin() * 0.15;

            let sample = (fundamental + harmonic_2 + harmonic_3) * amplitude * envelope;
            samples.push(sample);
        }

        samples
    }

    /// Smooth exponential envelope for natural attack/release
    fn smooth_envelope(&self, t: f32, duration: f32) -> f32 {
        let attack = 0.02; // 20ms attack (softer onset)
        let release = 0.12; // 120ms release (longer, smoother fade)

        let attack_env = if t < attack {
            // Exponential attack: 1 - e^(-t/tau)
            1.0 - (-t / (attack * 0.3)).exp()
        } else {
            1.0
        };

        let release_start = duration - release;
        let release_env = if t > release_start {
            // Exponential release: e^(-(t-start)/tau)
            let release_t = t - release_start;
            (-release_t / (release * 0.4)).exp()
        } else {
            1.0
        };

        attack_env * release_env
    }
}
