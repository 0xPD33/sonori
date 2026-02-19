#![allow(clippy::needless_range_loop)]

use ndarray::{ArrayD, IxDyn};
use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::Arc;

const SAMPLE_RATE: f32 = 16000.0;
const FFT_SIZE: usize = 512;
const WINDOW_SIZE: usize = 400;
const HOP_SIZE: usize = 160;
const N_MELS: usize = 128;
const PREEMPHASIS: f32 = 0.97;

pub struct MelSpectrogram {
    fft_size: usize,
    window_size: usize,
    hop_size: usize,
    n_mels: usize,
    preemphasis: f32,
    hann_window: Vec<f32>,
    mel_filterbank: Vec<Vec<f32>>, // [n_mels][fft_size/2 + 1]
    fft: Arc<dyn rustfft::Fft<f32>>,
}

fn hz_to_mel(hz: f32) -> f32 {
    if hz < 1000.0 {
        hz / 200.0 * 3.0
    } else {
        15.0 + (hz / 1000.0).ln() * (27.0 / (6400.0f32 / 1000.0).ln())
    }
}

fn mel_to_hz(mel: f32) -> f32 {
    if mel < 15.0 {
        mel * 200.0 / 3.0
    } else {
        1000.0 * ((mel - 15.0) * (6400.0f32 / 1000.0).ln() / 27.0).exp()
    }
}

fn build_mel_filterbank(n_mels: usize, fft_size: usize, sample_rate: f32) -> Vec<Vec<f32>> {
    let num_fft_bins = fft_size / 2 + 1;
    let f_min = 0.0f32;
    let f_max = sample_rate / 2.0;

    let mel_min = hz_to_mel(f_min);
    let mel_max = hz_to_mel(f_max);

    // n_mels + 2 equally spaced points in mel scale
    let mel_points: Vec<f32> = (0..n_mels + 2)
        .map(|i| mel_min + (mel_max - mel_min) * i as f32 / (n_mels + 1) as f32)
        .collect();

    let hz_points: Vec<f32> = mel_points.iter().map(|&m| mel_to_hz(m)).collect();

    // Convert Hz to FFT bin indices (floating point)
    let bin_points: Vec<f32> = hz_points
        .iter()
        .map(|&hz| hz * fft_size as f32 / sample_rate)
        .collect();

    let mut filterbank = vec![vec![0.0f32; num_fft_bins]; n_mels];

    for (m, filter) in filterbank.iter_mut().enumerate() {
        let f_left = bin_points[m];
        let f_center = bin_points[m + 1];
        let f_right = bin_points[m + 2];

        for (k, weight) in filter.iter_mut().enumerate() {
            let k_f = k as f32;
            if k_f >= f_left && k_f <= f_center && f_center > f_left {
                *weight = (k_f - f_left) / (f_center - f_left);
            } else if k_f > f_center && k_f <= f_right && f_right > f_center {
                *weight = (f_right - k_f) / (f_right - f_center);
            }
        }

        // Slaney normalization: normalize each filter to have unit area
        let enorm = 2.0 / (mel_to_hz(mel_points[m + 2]) - mel_to_hz(mel_points[m]));
        for weight in filter.iter_mut() {
            *weight *= enorm;
        }
    }

    filterbank
}

fn build_hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|n| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * n as f32 / size as f32).cos())
        })
        .collect()
}

impl Default for MelSpectrogram {
    fn default() -> Self {
        Self::new()
    }
}

impl MelSpectrogram {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let hann_window = build_hann_window(WINDOW_SIZE);
        let mel_filterbank = build_mel_filterbank(N_MELS, FFT_SIZE, SAMPLE_RATE);

        Self {
            fft_size: FFT_SIZE,
            window_size: WINDOW_SIZE,
            hop_size: HOP_SIZE,
            n_mels: N_MELS,
            preemphasis: PREEMPHASIS,
            hann_window,
            mel_filterbank,
            fft,
        }
    }

    pub fn compute(&self, samples: &[f32]) -> ArrayD<f32> {
        // 1. Preemphasis
        let mut emphasized = Vec::with_capacity(samples.len());
        if !samples.is_empty() {
            emphasized.push(samples[0]);
            for i in 1..samples.len() {
                emphasized.push(samples[i] - self.preemphasis * samples[i - 1]);
            }
        }

        // 2. Calculate number of frames
        let num_frames = if emphasized.len() >= self.window_size {
            (emphasized.len() - self.window_size) / self.hop_size + 1
        } else {
            0
        };

        if num_frames == 0 {
            return ArrayD::zeros(IxDyn(&[1, self.n_mels, 0]));
        }

        // 3. Frame, window, FFT, and mel filterbank
        let num_fft_bins = self.fft_size / 2 + 1;
        let mut mel_spec = vec![vec![0.0f32; num_frames]; self.n_mels];

        let mut fft_buffer: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); self.fft_size];
        for frame_idx in 0..num_frames {
            let start = frame_idx * self.hop_size;

            // Build windowed frame, zero-padded to fft_size
            for (i, buf) in fft_buffer.iter_mut().enumerate() {
                *buf = if i < self.window_size {
                    Complex::new(emphasized[start + i] * self.hann_window[i], 0.0)
                } else {
                    Complex::new(0.0, 0.0)
                };
            }

            // FFT
            self.fft.process(&mut fft_buffer);

            // Power spectrum (magnitude squared)
            let power: Vec<f32> = fft_buffer[..num_fft_bins]
                .iter()
                .map(|c| c.norm_sqr())
                .collect();

            // Apply mel filterbank
            for (m, filter) in self.mel_filterbank.iter().enumerate() {
                let sum: f32 = filter.iter().zip(power.iter()).map(|(w, p)| w * p).sum();
                mel_spec[m][frame_idx] = sum;
            }
        }

        // 5. Log mel spectrogram
        for bin in &mut mel_spec {
            for val in bin.iter_mut() {
                *val = (*val).max(1e-10).ln();
            }
        }

        // 6. Per-feature normalization (subtract mean, divide by std per mel bin)
        for bin in &mut mel_spec {
            let mean = bin.iter().copied().sum::<f32>() / num_frames as f32;
            let var = bin
                .iter()
                .map(|&v| {
                    let diff = v - mean;
                    diff * diff
                })
                .sum::<f32>()
                / num_frames as f32;
            let std = var.sqrt().max(1e-10);
            for val in bin.iter_mut() {
                *val = (*val - mean) / std;
            }
        }

        // 7. Reshape to [1, 128, T]
        let mut data = Vec::with_capacity(self.n_mels * num_frames);
        for bin in &mel_spec {
            data.extend_from_slice(bin);
        }

        ArrayD::from_shape_vec(IxDyn(&[1, self.n_mels, num_frames]), data)
            .expect("mel spectrogram shape mismatch")
    }
}
