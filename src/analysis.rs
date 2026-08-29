//! Fixed-size realtime spectrum analysis shared by both VST3 device classes.

use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex};

use crate::constants::{
    FFT_HOP_SIZE, FFT_SIZE, MAX_BANDS, MAX_FREQUENCY_HZ, MAX_LEVEL_DB, MIN_FREQUENCY_HZ,
    MIN_LEVEL_DB, SPECTRAL_TILT_DB_PER_OCTAVE, SPECTRAL_TILT_REFERENCE_HZ, SPECTRUM_ATTACK_SECONDS,
    SPECTRUM_RELEASE_SECONDS,
};

/// One published log-spaced spectrum frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpectrumFrame {
    /// Monotonically increasing mailbox sequence for this frame.
    pub sequence: u64,
    /// Exclusive end sample of the analysis window on the host timeline.
    ///
    /// `Some` means the host supplied a usable process-context sample
    /// position. `None` is an explicitly untimed fallback; it must not be
    /// treated as comparable across plugin instances.
    pub sample_position: Option<i64>,
    /// Display values in decibels, from low to high frequency.
    pub values: [f32; MAX_BANDS],
}

impl Default for SpectrumFrame {
    fn default() -> Self {
        Self {
            sequence: 0,
            sample_position: None,
            values: [MIN_LEVEL_DB; MAX_BANDS],
        }
    }
}

/// Preallocated FFT analyzer intended to be owned by one audio processor.
///
/// Construction and sample-rate changes happen outside the audio callback.
/// [`process_stereo_block`](Self::process_stereo_block) only mutates fixed-size
/// history and already allocated FFT buffers. The display tilt and ballistics
/// are applied here, after spectral measurement; they never affect audio
/// passthrough.
pub struct SpectrumAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    left_fft_buffer: Vec<Complex<f32>>,
    right_fft_buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    left_history: [f32; FFT_SIZE],
    right_history: [f32; FFT_SIZE],
    window: [f32; FFT_SIZE],
    window_sum: f32,
    write_index: usize,
    filled: usize,
    samples_since_frame: usize,
    sample_rate_hz: f32,
    values: [f32; MAX_BANDS],
    has_frame: bool,
    latest_sample_position: Option<i64>,
    expected_host_sample: Option<i64>,
}

impl SpectrumAnalyzer {
    /// Create an analyzer with a validated sample rate.
    pub fn new(sample_rate_hz: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch_len = fft.get_inplace_scratch_len();
        let window = std::array::from_fn(|index| {
            let phase = std::f32::consts::TAU * index as f32 / (FFT_SIZE - 1) as f32;
            0.5 - 0.5 * phase.cos()
        });
        let window_sum = window.iter().sum();

        Self {
            fft,
            left_fft_buffer: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            right_fft_buffer: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            scratch: vec![Complex::new(0.0, 0.0); scratch_len],
            left_history: [0.0; FFT_SIZE],
            right_history: [0.0; FFT_SIZE],
            window,
            window_sum,
            write_index: 0,
            filled: 0,
            samples_since_frame: 0,
            sample_rate_hz: valid_sample_rate(sample_rate_hz),
            values: [MIN_LEVEL_DB; MAX_BANDS],
            has_frame: false,
            latest_sample_position: None,
            expected_host_sample: None,
        }
    }

    /// Update the sample rate before processing begins or after a host reset.
    pub fn set_sample_rate_hz(&mut self, sample_rate_hz: f32) {
        self.sample_rate_hz = valid_sample_rate(sample_rate_hz);
    }

    /// Process a host block as dual-mono without allocating or taking a lock.
    pub fn process_block(&mut self, samples: &[f32]) -> bool {
        self.process_stereo_block_at(samples, samples, None)
    }

    /// Process a stereo host block without allocation or a phase-canceling
    /// mono fold. Each channel is transformed independently and their powers
    /// are averaged per frequency bin, so opposite-polarity stereo content is
    /// still represented.
    pub fn process_stereo_block(&mut self, left: &[f32], right: &[f32]) -> bool {
        self.process_stereo_block_at(left, right, None)
    }

    /// Process a stereo host block with an optional host-timeline start.
    ///
    /// When a sample position is present, the analyzer rejects discontinuous
    /// blocks and stamps each published frame with the exclusive end sample of
    /// its 8192-sample window. Without one, it still analyzes the audio but
    /// marks the resulting frame untimed so the viewer uses the latest-frame
    /// fallback instead of inventing cross-instance alignment.
    pub fn process_stereo_block_at(
        &mut self,
        left: &[f32],
        right: &[f32],
        block_start_sample: Option<i64>,
    ) -> bool {
        let sample_count = left.len().min(right.len());
        let mut published = false;
        match block_start_sample {
            Some(block_start) => {
                if self.expected_host_sample != Some(block_start) {
                    self.reset();
                }
                for index in 0..sample_count {
                    let sample_position = block_start.saturating_add(index as i64);
                    if self.push_sample(left[index], right[index], Some(sample_position)) {
                        published = true;
                    }
                }
                self.expected_host_sample = Some(block_start.saturating_add(sample_count as i64));
            }
            None => {
                if self.expected_host_sample.is_some() {
                    self.reset();
                }
                for (left, right) in left.iter().zip(right.iter()) {
                    if self.push_sample(*left, *right, None) {
                        published = true;
                    }
                }
                self.expected_host_sample = None;
            }
        }
        published
    }

    /// Return the latest computed display values for publication.
    pub fn latest_values(&self) -> &[f32; MAX_BANDS] {
        &self.values
    }

    /// Return the latest frame, if the analyzer has received a full FFT window.
    pub fn latest_frame(&self) -> Option<SpectrumFrame> {
        self.has_frame.then_some(SpectrumFrame {
            sequence: 0,
            sample_position: self.latest_sample_position,
            values: self.values,
        })
    }

    /// Clear accumulated history after analysis demand has been withdrawn.
    ///
    /// This only writes already allocated fixed-size storage and is intended
    /// for the audio processor's demand transition, never for a control lock.
    pub fn reset(&mut self) {
        self.left_history.fill(0.0);
        self.right_history.fill(0.0);
        self.left_fft_buffer.fill(Complex::new(0.0, 0.0));
        self.right_fft_buffer.fill(Complex::new(0.0, 0.0));
        self.scratch.fill(Complex::new(0.0, 0.0));
        self.values.fill(MIN_LEVEL_DB);
        self.write_index = 0;
        self.filled = 0;
        self.samples_since_frame = 0;
        self.has_frame = false;
        self.latest_sample_position = None;
        self.expected_host_sample = None;
    }

    fn push_sample(&mut self, left: f32, right: f32, sample_position: Option<i64>) -> bool {
        self.left_history[self.write_index] = finite_or_zero(left);
        self.right_history[self.write_index] = finite_or_zero(right);
        self.write_index += 1;
        if self.write_index == FFT_SIZE {
            self.write_index = 0;
        }
        self.filled = (self.filled + 1).min(FFT_SIZE);
        self.samples_since_frame += 1;

        if self.filled == FFT_SIZE && self.samples_since_frame >= FFT_HOP_SIZE {
            self.samples_since_frame = 0;
            self.compute_frame(sample_position.map(|position| position.saturating_add(1)));
            return true;
        }
        false
    }

    fn compute_frame(&mut self, sample_position: Option<i64>) {
        for index in 0..FFT_SIZE {
            let history_index = (self.write_index + index) % FFT_SIZE;
            let window = self.window[index];
            self.left_fft_buffer[index] =
                Complex::new(self.left_history[history_index] * window, 0.0);
            self.right_fft_buffer[index] =
                Complex::new(self.right_history[history_index] * window, 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.left_fft_buffer, &mut self.scratch);
        self.fft
            .process_with_scratch(&mut self.right_fft_buffer, &mut self.scratch);

        let power_scale = (2.0 / self.window_sum).powi(2);
        for band in 0..MAX_BANDS {
            let (start_bin, end_bin) = self.band_bin_bounds(band);
            let mut sum_power = 0.0;
            let mut count = 0usize;
            for bin in start_bin..=end_bin {
                let left_power = self.left_fft_buffer[bin].norm_sqr();
                let right_power = self.right_fft_buffer[bin].norm_sqr();
                sum_power += (left_power + right_power) * 0.5 * power_scale;
                count += 1;
            }

            let power = if count == 0 {
                0.0
            } else {
                sum_power / count as f32
            };
            let raw_db = 10.0 * power.max(1.0e-20).log10();
            let frequency = band_center_frequency_hz(band, self.sample_rate_hz);
            let display_db = display_level_db(raw_db, frequency);
            self.values[band] = if self.has_frame {
                smooth_db(
                    self.values[band],
                    display_db,
                    FFT_HOP_SIZE as f32 / self.sample_rate_hz,
                )
            } else {
                display_db
            };
        }
        self.has_frame = true;
        self.latest_sample_position = sample_position;
    }

    fn band_bin_bounds(&self, band: usize) -> (usize, usize) {
        let max_frequency = MAX_FREQUENCY_HZ
            .min(self.sample_rate_hz * 0.475)
            .max(MIN_FREQUENCY_HZ * 1.01);
        let ratio = max_frequency / MIN_FREQUENCY_HZ;
        let low_frequency = MIN_FREQUENCY_HZ * ratio.powf(band as f32 / MAX_BANDS as f32);
        let high_frequency = MIN_FREQUENCY_HZ * ratio.powf((band + 1) as f32 / MAX_BANDS as f32);
        let maximum_bin = FFT_SIZE / 2;
        let start = ((low_frequency / self.sample_rate_hz) * FFT_SIZE as f32)
            .round()
            .clamp(1.0, maximum_bin as f32) as usize;
        let end = ((high_frequency / self.sample_rate_hz) * FFT_SIZE as f32)
            .round()
            .max(start as f32)
            .min(maximum_bin as f32) as usize;
        (start, end)
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn valid_sample_rate(sample_rate_hz: f32) -> f32 {
    if sample_rate_hz.is_finite() && sample_rate_hz > 1.0 {
        sample_rate_hz
    } else {
        48_000.0
    }
}

/// Return the logarithmic center frequency for one display band.
pub fn band_center_frequency_hz(band: usize, sample_rate_hz: f32) -> f32 {
    let max_frequency = MAX_FREQUENCY_HZ
        .min(valid_sample_rate(sample_rate_hz) * 0.475)
        .max(MIN_FREQUENCY_HZ * 1.01);
    let ratio = max_frequency / MIN_FREQUENCY_HZ;
    MIN_FREQUENCY_HZ * ratio.powf((band as f32 + 0.5) / MAX_BANDS as f32)
}

/// Return the display-only tilt applied to a frequency in dB.
pub(crate) fn display_tilt_db(frequency_hz: f32) -> f32 {
    SPECTRAL_TILT_DB_PER_OCTAVE
        * (frequency_hz.max(MIN_FREQUENCY_HZ) / SPECTRAL_TILT_REFERENCE_HZ).log2()
}

/// Convert one raw spectral level into the plotted level.
///
/// The display floor is a semantic gate: bins at or below `MIN_LEVEL_DB` are
/// rendered as a flat floor and do not receive the presentation tilt. Real
/// signals above that floor retain the configured tilt, including low-level
/// signals, while silence and numerical noise cannot become a diagonal line.
pub(crate) fn display_level_db(raw_db: f32, frequency_hz: f32) -> f32 {
    if !raw_db.is_finite() || raw_db <= MIN_LEVEL_DB {
        MIN_LEVEL_DB
    } else {
        (raw_db + display_tilt_db(frequency_hz)).clamp(MIN_LEVEL_DB, MAX_LEVEL_DB)
    }
}

/// Apply one dB-domain exponential ballistics step.
pub(crate) fn smooth_db(previous_db: f32, target_db: f32, frame_seconds: f32) -> f32 {
    let time_constant = if target_db >= previous_db {
        SPECTRUM_ATTACK_SECONDS
    } else {
        SPECTRUM_RELEASE_SECONDS
    };
    let response = 1.0 - (-frame_seconds.max(0.0) / time_constant).exp();
    previous_db + (target_db - previous_db) * response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(sample_rate: f32, frequency: f32, length: usize, amplitude: f32) -> Vec<f32> {
        (0..length)
            .map(|index| {
                amplitude * (std::f32::consts::TAU * frequency * index as f32 / sample_rate).sin()
            })
            .collect()
    }

    fn peak_band(frame: SpectrumFrame, sample_rate: f32) -> usize {
        frame
            .values
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(index, _)| {
                let frequency = band_center_frequency_hz(index, sample_rate);
                assert!((700.0..=1_400.0).contains(&frequency));
                index
            })
            .expect("spectrum has bands")
    }

    #[test]
    fn analyzer_uses_the_requested_8192_sample_fft_and_waits_for_a_window() {
        assert_eq!(FFT_SIZE, 8192);
        assert_eq!(FFT_HOP_SIZE, 2048);
        let mut analyzer = SpectrumAnalyzer::new(48_000.0);
        assert!(!analyzer.process_block(&[0.0; FFT_SIZE - 1]));
        assert!(analyzer.process_block(&[0.0; FFT_HOP_SIZE]));
        let frame = analyzer.latest_frame().expect("full window should publish");
        assert!(frame.values.iter().all(|value| value.is_finite()));
        assert!(frame.values.iter().all(|value| *value <= MAX_LEVEL_DB));
    }

    #[test]
    fn analyzer_places_a_one_kilohertz_tone_in_the_expected_band() {
        let sample_rate = 48_000.0;
        let mut analyzer = SpectrumAnalyzer::new(sample_rate);
        let samples = tone(sample_rate, 1_000.0, FFT_SIZE + FFT_HOP_SIZE, 1.0);
        assert!(analyzer.process_block(&samples));
        peak_band(
            analyzer.latest_frame().expect("tone should publish"),
            sample_rate,
        );
    }

    #[test]
    fn stereo_energy_keeps_opposite_polarity_channels_in_the_spectrum() {
        let sample_rate = 48_000.0;
        let tone = tone(sample_rate, 1_000.0, FFT_SIZE + FFT_HOP_SIZE, 1.0);
        let inverted: Vec<f32> = tone.iter().map(|sample| -*sample).collect();
        let mut stereo = SpectrumAnalyzer::new(sample_rate);
        let mut mono_fold = SpectrumAnalyzer::new(sample_rate);
        assert!(stereo.process_stereo_block(&tone, &inverted));
        let folded: Vec<f32> = tone
            .iter()
            .zip(inverted.iter())
            .map(|(left, right)| (left + right) * 0.5)
            .collect();
        assert!(mono_fold.process_block(&folded));

        let stereo_frame = stereo.latest_frame().expect("stereo input should publish");
        let folded_frame = mono_fold
            .latest_frame()
            .expect("folded input should publish");
        let band = peak_band(stereo_frame, sample_rate);
        assert!(stereo_frame.values[band] > folded_frame.values[band] + 30.0);
    }

    #[test]
    fn equal_left_and_right_energy_matches_single_channel_energy() {
        let sample_rate = 48_000.0;
        let left = tone(sample_rate, 1_000.0, FFT_SIZE + FFT_HOP_SIZE, 0.5);
        let right = tone(sample_rate, 1_000.0, FFT_SIZE + FFT_HOP_SIZE, 0.5);
        let mut stereo = SpectrumAnalyzer::new(sample_rate);
        let mut single_channel = SpectrumAnalyzer::new(sample_rate);
        assert!(stereo.process_stereo_block(&left, &right));
        assert!(single_channel.process_block(&left));
        let stereo_frame = stereo.latest_frame().expect("stereo input should publish");
        let single_frame = single_channel
            .latest_frame()
            .expect("single channel input should publish");
        let band = peak_band(stereo_frame, sample_rate);
        assert!((stereo_frame.values[band] - single_frame.values[band]).abs() < 0.01);
    }

    #[test]
    fn display_tilt_is_zero_at_reference_and_four_point_five_per_octave() {
        assert!((display_tilt_db(1_000.0)).abs() < 1.0e-6);
        assert!((display_tilt_db(2_000.0) - 4.5).abs() < 1.0e-6);
        assert!((display_tilt_db(500.0) + 4.5).abs() < 1.0e-6);
    }

    #[test]
    fn silence_is_flat_at_the_floor_while_a_real_low_level_signal_keeps_tilt() {
        let mut analyzer = SpectrumAnalyzer::new(48_000.0);
        let samples = vec![0.0; FFT_SIZE + FFT_HOP_SIZE];
        assert!(analyzer.process_stereo_block(&samples, &samples));
        let frame = analyzer
            .latest_frame()
            .expect("silence should produce a frame");
        assert!(frame.values.iter().all(|value| *value == MIN_LEVEL_DB));

        assert_eq!(display_level_db(MIN_LEVEL_DB, 20_000.0), MIN_LEVEL_DB);
        assert_eq!(display_level_db(-120.0, 20_000.0), MIN_LEVEL_DB);
        let low_level = display_level_db(-99.0, 20_000.0);
        assert!((low_level - (-99.0 + display_tilt_db(20_000.0))).abs() < 1.0e-5);
        assert!(low_level > -99.0);
    }

    #[test]
    fn timed_analysis_frames_use_exclusive_window_end_and_reset_on_discontinuity() {
        let mut analyzer = SpectrumAnalyzer::new(48_000.0);
        let samples = vec![0.0; FFT_SIZE + FFT_HOP_SIZE];
        let first_start = 12_000_i64;
        assert!(analyzer.process_stereo_block_at(&samples, &samples, Some(first_start)));
        assert_eq!(
            analyzer
                .latest_frame()
                .expect("timed frame should be available")
                .sample_position,
            Some(first_start + samples.len() as i64)
        );

        let discontinuous_start = first_start + samples.len() as i64 + 17;
        assert!(analyzer.process_stereo_block_at(&samples, &samples, Some(discontinuous_start)));
        assert_eq!(
            analyzer
                .latest_frame()
                .expect("post-reset timed frame should be available")
                .sample_position,
            Some(discontinuous_start + samples.len() as i64)
        );
    }

    #[test]
    fn smoothing_attacks_faster_than_it_releases_and_decay_is_monotonic() {
        let frame_seconds = FFT_HOP_SIZE as f32 / 48_000.0;
        let rising = smooth_db(-60.0, 0.0, frame_seconds);
        let falling = smooth_db(0.0, -60.0, frame_seconds);
        assert!(rising - (-60.0) > falling.abs());

        let mut value = 0.0;
        let mut previous = value;
        for _ in 0..8 {
            value = smooth_db(value, -60.0, frame_seconds);
            assert!(value < previous);
            previous = value;
        }
        assert!(value > -60.0);
    }

    #[test]
    fn invalid_sample_rate_uses_a_safe_default() {
        let mut analyzer = SpectrumAnalyzer::new(f32::NAN);
        analyzer.process_block(&vec![0.0; FFT_SIZE + FFT_HOP_SIZE]);
        assert!(analyzer.latest_frame().is_some());
    }
}
