//! Bounded UI-side pacing for already aligned spectrum frames.

use crate::analysis::SpectrumFrame;
use crate::constants::{
    MAX_BANDS, MAX_COMPANIONS, MIN_LEVEL_DB, PRESENTATION_ATTACK_SECONDS,
    PRESENTATION_MAX_STEP_SECONDS, PRESENTATION_RELEASE_SECONDS,
};

/// One fixed-size temporal smoother for a displayed trace.
#[derive(Clone)]
pub(crate) struct PresentationBallistics {
    values: [f32; MAX_BANDS],
    initialized: bool,
}

impl Default for PresentationBallistics {
    fn default() -> Self {
        Self {
            values: [MIN_LEVEL_DB; MAX_BANDS],
            initialized: false,
        }
    }
}

impl PresentationBallistics {
    /// Advance toward one current analysis frame using a bounded UI step.
    pub(crate) fn advance(&mut self, target: SpectrumFrame, elapsed_seconds: f32) -> SpectrumFrame {
        if !self.initialized {
            self.values = target.values;
            self.initialized = true;
        } else {
            let elapsed_seconds = bounded_elapsed_seconds(elapsed_seconds);
            for (current, target) in self.values.iter_mut().zip(target.values) {
                *current = smooth_display_db(*current, target, elapsed_seconds);
            }
        }
        SpectrumFrame {
            sequence: target.sequence,
            sample_position: target.sample_position,
            values: self.values,
        }
    }
}

/// Bound one host repaint interval before applying presentation ballistics.
pub(crate) fn bounded_elapsed_seconds(elapsed_seconds: f32) -> f32 {
    elapsed_seconds.clamp(0.0, PRESENTATION_MAX_STEP_SECONDS)
}

/// Apply one display-domain attack/release step.
pub(crate) fn smooth_display_db(previous_db: f32, target_db: f32, elapsed_seconds: f32) -> f32 {
    let previous_db = finite_or_floor(previous_db);
    let target_db = finite_or_floor(target_db);
    let time_constant = if target_db >= previous_db {
        PRESENTATION_ATTACK_SECONDS
    } else {
        PRESENTATION_RELEASE_SECONDS
    };
    let response = 1.0 - (-elapsed_seconds.max(0.0) / time_constant).exp();
    previous_db + (target_db - previous_db) * response
}

fn finite_or_floor(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        MIN_LEVEL_DB
    }
}

/// One selected companion's retained presentation state.
struct PresentationEntry {
    id: u64,
    smoother: PresentationBallistics,
}

/// UI-side presentation state for the main trace and selected companions.
///
/// This state is never accessed by an audio callback. Entries are created only
/// for selected sources and are bounded by the registry capacity.
pub(crate) struct PresentationState {
    main: PresentationBallistics,
    companions: Vec<PresentationEntry>,
}

impl Default for PresentationState {
    fn default() -> Self {
        Self {
            main: PresentationBallistics::default(),
            companions: Vec::with_capacity(MAX_COMPANIONS),
        }
    }
}

impl PresentationState {
    /// Advance the main trace, if a frame is available.
    pub(crate) fn advance_main(
        &mut self,
        frame: Option<SpectrumFrame>,
        elapsed_seconds: f32,
    ) -> Option<SpectrumFrame> {
        frame.map(|frame| self.main.advance(frame, elapsed_seconds))
    }

    /// Advance one selected companion trace, creating only its bounded UI
    /// state on first use.
    pub(crate) fn advance_companion(
        &mut self,
        id: u64,
        frame: Option<SpectrumFrame>,
        elapsed_seconds: f32,
    ) -> Option<SpectrumFrame> {
        let frame = frame?;
        let index = match self.companions.iter().position(|entry| entry.id == id) {
            Some(index) => index,
            None => {
                if self.companions.len() >= MAX_COMPANIONS {
                    return None;
                }
                self.companions.push(PresentationEntry {
                    id,
                    smoother: PresentationBallistics::default(),
                });
                self.companions.len() - 1
            }
        };
        Some(
            self.companions[index]
                .smoother
                .advance(frame, elapsed_seconds),
        )
    }

    /// Drop presentation state for sources that are no longer selected.
    pub(crate) fn retain_selected(&mut self, selected_ids: &[u64]) {
        self.companions
            .retain(|entry| selected_ids.contains(&entry.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{
        ALIGNMENT_MAX_SKEW_SAMPLES, PRESENTATION_FRAME_SECONDS, PRESENTATION_TARGET_FPS,
    };

    fn frame(value: f32) -> SpectrumFrame {
        SpectrumFrame {
            sequence: 1,
            sample_position: Some(ALIGNMENT_MAX_SKEW_SAMPLES),
            values: [value; MAX_BANDS],
        }
    }

    #[test]
    fn first_presentation_frame_is_immediate() {
        let mut smoother = PresentationBallistics::default();
        let presented = smoother.advance(frame(-12.0), 0.0);
        assert_eq!(presented.values, [-12.0; MAX_BANDS]);
    }

    #[test]
    fn presentation_release_is_monotonic_and_attack_is_quicker() {
        let mut smoother = PresentationBallistics::default();
        smoother.advance(frame(-60.0), 0.0);
        let attack = smoother.advance(frame(0.0), 1.0 / 60.0).values[0] + 60.0;

        let mut smoother = PresentationBallistics::default();
        smoother.advance(frame(0.0), 0.0);
        let release_first = smoother.advance(frame(-60.0), 1.0 / 60.0).values[0];
        let mut previous = release_first;
        for _ in 0..8 {
            let value = smoother.advance(frame(-60.0), 1.0 / 60.0).values[0];
            assert!(value < previous);
            previous = value;
        }
        assert!(attack > release_first.abs());
    }

    #[test]
    fn jittered_repaint_intervals_match_regular_elapsed_time() {
        let mut regular = PresentationBallistics::default();
        let mut jittered = PresentationBallistics::default();
        regular.advance(frame(-60.0), 0.0);
        jittered.advance(frame(-60.0), 0.0);
        for _ in 0..12 {
            regular.advance(frame(0.0), PRESENTATION_FRAME_SECONDS);
        }
        for elapsed in [
            0.011, 0.024, 0.013, 0.019, 0.016, 0.017, 0.014, 0.020, 0.012, 0.021, 0.015, 0.018,
        ] {
            jittered.advance(frame(0.0), elapsed);
        }
        assert!(
            (regular.advance(frame(0.0), 0.0).values[0]
                - jittered.advance(frame(0.0), 0.0).values[0])
                .abs()
                < 0.01
        );
    }

    #[test]
    fn delayed_repaint_step_is_bounded() {
        let mut smoother = PresentationBallistics::default();
        smoother.advance(frame(-60.0), 0.0);
        let bounded = smoother.advance(frame(0.0), 1.0).values[0];
        assert!(bounded < 0.0);
    }

    #[test]
    fn presentation_contract_targets_at_least_sixty_hz_and_bounds_jitter() {
        const {
            assert!(PRESENTATION_TARGET_FPS >= 60);
        }
        assert!((PRESENTATION_FRAME_SECONDS - 1.0 / 60.0).abs() < 1.0e-6);
        assert_eq!(bounded_elapsed_seconds(-1.0), 0.0);
        assert_eq!(bounded_elapsed_seconds(1.0), PRESENTATION_MAX_STEP_SECONDS);
    }
}
