//! Common-time selection for main and companion display frames.

use crate::analysis::SpectrumFrame;
use crate::constants::ALIGNMENT_MAX_SKEW_SAMPLES;
use crate::registry::CompanionSourceSnapshot;

/// Choose a common analysis time anchored to the viewer's latest timed frame.
///
/// A selected companion contributes its latest timed position only when it is
/// within the bounded scheduling skew of the main frame. The target is the
/// oldest eligible position, which lets each mailbox choose a nearby frame
/// without dragging the graph back to a genuinely stale source. Untimed
/// frames do not affect the target and use the latest-frame fallback.
pub(crate) fn common_target_sample(
    main: Option<SpectrumFrame>,
    companions: &[CompanionSourceSnapshot],
    selected_ids: &[u64],
) -> Option<i64> {
    let main_sample = main.and_then(|frame| frame.sample_position)?;
    let mut target = main_sample;
    for source in companions {
        if !source.active || !source.analysis_requested || !selected_ids.contains(&source.id) {
            continue;
        }
        let Some(sample_position) = source.frame.and_then(|frame| frame.sample_position) else {
            continue;
        };
        if sample_position.abs_diff(main_sample) <= ALIGNMENT_MAX_SKEW_SAMPLES as u64 {
            target = target.min(sample_position);
        }
    }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::MAX_BANDS;
    use crate::constants::Rgb;
    use crate::registry::SpectrumMailbox;

    fn frame(sequence: u64, sample_position: Option<i64>) -> SpectrumFrame {
        SpectrumFrame {
            sequence,
            sample_position,
            values: [-24.0; MAX_BANDS],
        }
    }

    fn source(id: u64, sample_position: Option<i64>) -> CompanionSourceSnapshot {
        CompanionSourceSnapshot {
            id,
            name: format!("COMPANION {id}"),
            color: Rgb::new(100, 120, 140),
            active: true,
            analysis_requested: true,
            frame: Some(frame(id, sample_position)),
        }
    }

    #[test]
    fn common_target_uses_nearby_frames_but_ignores_stale_sources() {
        let main = frame(1, Some(10_000));
        let companions = [
            source(2, Some(7_952)),
            source(3, Some(12_048)),
            source(4, Some(3_000)),
        ];

        assert_eq!(
            common_target_sample(Some(main), &companions, &[2, 3, 4]),
            Some(7_952)
        );
    }

    #[test]
    fn untimed_main_frame_uses_the_explicit_latest_frame_fallback() {
        let companions = [source(2, Some(10_000))];
        assert_eq!(
            common_target_sample(Some(frame(1, None)), &companions, &[2]),
            None
        );
    }

    #[test]
    fn unselected_and_inactive_sources_do_not_move_the_common_target() {
        let main = frame(1, Some(10_000));
        let mut inactive = source(2, Some(7_952));
        inactive.active = false;
        let unselected = source(3, Some(7_952));
        assert_eq!(
            common_target_sample(Some(main), &[inactive, unselected], &[2]),
            Some(10_000)
        );
    }

    #[test]
    fn same_source_history_converges_main_and_companion_to_one_analysis_time() {
        let main_mailbox = SpectrumMailbox::new();
        let companion_mailbox = SpectrumMailbox::new();
        main_mailbox.publish_frame(&frame(1, Some(9_000)));
        main_mailbox.publish_frame(&frame(2, Some(10_000)));
        companion_mailbox.publish_frame(&frame(3, Some(10_000)));
        companion_mailbox.publish_frame(&frame(4, Some(12_048)));

        let main_latest = main_mailbox.read().expect("main frame should exist");
        let companion_latest = companion_mailbox
            .read()
            .expect("companion frame should exist");
        let companions = [CompanionSourceSnapshot {
            id: 2,
            name: "COMPANION 2".to_string(),
            color: Rgb::new(100, 120, 140),
            active: true,
            analysis_requested: true,
            frame: Some(companion_latest),
        }];
        let target = common_target_sample(Some(main_latest), &companions, &[2])
            .expect("timed sources should have a common target");
        let aligned_main = main_mailbox
            .read_nearest(target, ALIGNMENT_MAX_SKEW_SAMPLES)
            .expect("main history should contain the target frame");
        let aligned_companion = companion_mailbox
            .read_nearest(target, ALIGNMENT_MAX_SKEW_SAMPLES)
            .expect("companion history should contain the target frame");

        assert_eq!(target, 10_000);
        assert_eq!(
            aligned_main.sample_position,
            aligned_companion.sample_position
        );
        assert_eq!(aligned_main.sample_position, Some(target));
    }
}
