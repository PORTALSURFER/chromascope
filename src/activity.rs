//! Realtime-safe companion input-activity detection.
//!
//! This meter describes only pre-fader samples arriving at a companion device.
//! It is intentionally not a channel-volume, mute, or final-audibility meter.
//! The audio side keeps the small envelope state locally and publishes only
//! the hysteresis result through the companion registry's atomic flag.

/// Amplitude above which a quiet companion is considered to have input.
///
/// This is approximately -60 dBFS. It is high enough to avoid showing a
/// digital noise floor as activity while still detecting ordinary quiet input.
const ACTIVITY_ON_THRESHOLD: f32 = 1.0e-3;
/// Amplitude below which an active companion is considered silent.
///
/// The gap below [`ACTIVITY_ON_THRESHOLD`] prevents the marker from chattering
/// around the gate threshold.
const ACTIVITY_OFF_THRESHOLD: f32 = 5.0e-4;
/// Envelope attack time for an incoming companion block.
const ACTIVITY_ATTACK_SECONDS: f32 = 0.005;
/// Envelope release time after the companion input becomes quiet.
const ACTIVITY_RELEASE_SECONDS: f32 = 0.100;

/// Audio-owned, bounded envelope and hysteresis state for one companion.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct InputActivityMeter {
    envelope: f32,
    active: bool,
}

impl InputActivityMeter {
    /// Update the meter with one process block and return its gated state.
    ///
    /// The caller supplies a block peak so this performs one bounded envelope
    /// update per callback rather than doing additional cross-thread work per
    /// sample. Non-finite peaks and invalid sample rates are treated as quiet.
    pub(crate) fn process_block(
        &mut self,
        block_peak: f32,
        sample_count: usize,
        sample_rate_hz: f32,
    ) -> bool {
        let target = if block_peak.is_finite() {
            block_peak.abs().clamp(0.0, 1.0)
        } else {
            0.0
        };
        let sample_rate = if sample_rate_hz.is_finite() && sample_rate_hz > 1.0 {
            sample_rate_hz
        } else {
            48_000.0
        };
        let block_seconds = sample_count as f32 / sample_rate;
        let time_constant = if target >= self.envelope {
            ACTIVITY_ATTACK_SECONDS
        } else {
            ACTIVITY_RELEASE_SECONDS
        };
        let response = 1.0 - (-block_seconds.max(0.0) / time_constant).exp();
        self.envelope += (target - self.envelope) * response;

        if self.active {
            if self.envelope <= ACTIVITY_OFF_THRESHOLD {
                self.active = false;
            }
        } else if self.envelope >= ACTIVITY_ON_THRESHOLD {
            self.active = true;
        }
        self.active
    }

    /// Clear the envelope when the host stops processing this companion.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_requires_a_meaningful_peak_and_uses_hysteresis() {
        let mut meter = InputActivityMeter::default();

        assert!(!meter.process_block(0.0, 480, 48_000.0));
        assert!(!meter.process_block(ACTIVITY_ON_THRESHOLD * 0.9, 480, 48_000.0));
        assert!(meter.process_block(0.01, 480, 48_000.0));
        assert!(meter.process_block(ACTIVITY_OFF_THRESHOLD * 1.1, 480, 48_000.0));
    }

    #[test]
    fn activity_releases_after_sustained_silence() {
        let mut meter = InputActivityMeter::default();
        assert!(meter.process_block(1.0, 4_800, 48_000.0));

        let mut active = true;
        for _ in 0..32 {
            active = meter.process_block(0.0, 4_800, 48_000.0);
        }
        assert!(!active);
    }

    #[test]
    fn invalid_peak_and_sample_rate_are_safely_quiet_and_finite() {
        let mut meter = InputActivityMeter::default();
        assert!(!meter.process_block(f32::NAN, 480, f32::NAN));
        assert!(!meter.process_block(f32::INFINITY, 480, 0.0));
        meter.reset();
        assert!(!meter.process_block(0.0, 0, 48_000.0));
    }
}
