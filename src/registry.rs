//! Explicit process-local companion registry and lock-free spectrum mailboxes.
//!
//! The registry is control/UI state. It is intentionally locked only while a
//! companion is registered, host metadata is updated, or the viewer snapshots
//! the source list. The audio callback retains a handle to one slot and
//! communicates through atomics only.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::analysis::SpectrumFrame;
use crate::constants::{
    ALIGNMENT_MAX_SKEW_SAMPLES, FRAME_HISTORY_LEN, MAX_BANDS, MAX_COMPANIONS, Rgb,
};

/// Lock-free mailbox carrying one spectrum frame from an audio callback.
#[derive(Debug)]
pub struct SpectrumMailbox {
    next_sequence: AtomicU64,
    slots: [SpectrumMailboxSlot; FRAME_HISTORY_LEN],
}

/// One fixed-size seqlock slot in a spectrum mailbox.
#[derive(Debug)]
struct SpectrumMailboxSlot {
    sequence: AtomicU64,
    sample_position: AtomicI64,
    sample_position_valid: AtomicBool,
    values: [AtomicU32; MAX_BANDS],
}

impl SpectrumMailboxSlot {
    fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            sample_position: AtomicI64::new(0),
            sample_position_valid: AtomicBool::new(false),
            values: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
        }
    }
}

impl SpectrumMailbox {
    /// Create an empty mailbox with no published frame.
    pub fn new() -> Self {
        Self {
            next_sequence: AtomicU64::new(0),
            slots: std::array::from_fn(|_| SpectrumMailboxSlot::new()),
        }
    }

    /// Publish one complete frame without allocating or blocking.
    pub fn publish(&self, values: &[f32; MAX_BANDS]) -> u64 {
        self.publish_frame(&SpectrumFrame {
            sequence: 0,
            sample_position: None,
            values: *values,
        })
    }

    /// Publish one complete frame and its optional host-timeline position.
    ///
    /// The frame history is fixed-size and each slot uses a sequence guard, so
    /// audio callbacks perform only atomic stores and never allocate or lock.
    pub fn publish_frame(&self, frame: &SpectrumFrame) -> u64 {
        let published_sequence = self
            .next_sequence
            .fetch_add(2, Ordering::Relaxed)
            .wrapping_add(2);
        let slot_index = (published_sequence / 2) as usize % FRAME_HISTORY_LEN;
        let slot = &self.slots[slot_index];
        slot.sequence
            .store(published_sequence.wrapping_sub(1), Ordering::Release);
        for (destination, value) in slot.values.iter().zip(frame.values.iter().copied()) {
            destination.store(
                if value.is_finite() {
                    value.to_bits()
                } else {
                    0.0f32.to_bits()
                },
                Ordering::Relaxed,
            );
        }
        slot.sample_position
            .store(frame.sample_position.unwrap_or(0), Ordering::Relaxed);
        slot.sample_position_valid
            .store(frame.sample_position.is_some(), Ordering::Relaxed);
        slot.sequence.store(published_sequence, Ordering::Release);
        published_sequence
    }

    /// Copy a coherent frame into a fixed-size value when one is available.
    pub fn read(&self) -> Option<SpectrumFrame> {
        let mut latest = None;
        for slot in &self.slots {
            if let Some(candidate) = read_slot(slot) {
                let is_newer = latest
                    .as_ref()
                    .is_none_or(|current: &SpectrumFrame| candidate.sequence > current.sequence);
                if is_newer {
                    latest = Some(candidate);
                }
            }
        }
        latest
    }

    /// Read the frame nearest to a target sample position within a bounded
    /// skew. Ties prefer the frame at or before the target, then the newer
    /// mailbox sequence. Untimed frames are intentionally never selected.
    pub fn read_nearest(&self, target_sample: i64, max_skew_samples: i64) -> Option<SpectrumFrame> {
        let max_skew = max_skew_samples.max(0) as u64;
        let mut nearest: Option<(SpectrumFrame, u64)> = None;
        for slot in &self.slots {
            let Some(candidate) = read_slot(slot) else {
                continue;
            };
            let Some(sample_position) = candidate.sample_position else {
                continue;
            };
            let distance = sample_position.abs_diff(target_sample);
            if distance > max_skew {
                continue;
            }
            let replace = match nearest {
                None => true,
                Some((current, current_distance)) => {
                    distance < current_distance
                        || (distance == current_distance
                            && (sample_position <= target_sample)
                                != (current.sample_position.unwrap_or(i64::MAX) <= target_sample)
                            && sample_position <= target_sample)
                        || (distance == current_distance
                            && (sample_position <= target_sample)
                                == (current.sample_position.unwrap_or(i64::MAX) <= target_sample)
                            && candidate.sequence > current.sequence)
                }
            };
            if replace {
                nearest = Some((candidate, distance));
            }
        }
        nearest.map(|(frame, _)| frame)
    }
}

impl Default for SpectrumMailbox {
    fn default() -> Self {
        Self::new()
    }
}

fn read_slot(slot: &SpectrumMailboxSlot) -> Option<SpectrumFrame> {
    for _ in 0..4 {
        let start = slot.sequence.load(Ordering::Acquire);
        if start == 0 || start & 1 == 1 {
            std::hint::spin_loop();
            continue;
        }
        let values =
            std::array::from_fn(|index| f32::from_bits(slot.values[index].load(Ordering::Relaxed)));
        let sample_position = slot.sample_position.load(Ordering::Relaxed);
        let sample_position_valid = slot.sample_position_valid.load(Ordering::Relaxed);
        let end = slot.sequence.load(Ordering::Acquire);
        if start == end {
            return Some(SpectrumFrame {
                sequence: start,
                sample_position: sample_position_valid.then_some(sample_position),
                values,
            });
        }
    }
    None
}

/// A UI snapshot of one currently registered companion.
#[derive(Clone, Debug, PartialEq)]
pub struct CompanionSourceSnapshot {
    /// Stable identifier for the lifetime of the companion registration.
    pub id: u64,
    /// Host-provided track name, or the stable fallback companion label.
    pub name: String,
    /// Host-provided track color, or the stable generated fallback color.
    pub color: Rgb,
    /// Whether the companion currently has an active audio processor.
    pub active: bool,
    /// Whether at least one viewer has selected this source for analysis.
    ///
    /// An active but unrequested source remains discoverable in the list while
    /// its companion processor stays idle, avoiding FFT work merely because it
    /// is registered.
    pub analysis_requested: bool,
    /// Most recent coherent spectrum, or `None` while unavailable/warming up.
    pub frame: Option<SpectrumFrame>,
}

/// Internal UI snapshot of a companion's pre-fader input activity.
///
/// This remains separate from [`CompanionSourceSnapshot`] so adding the
/// diagnostic marker does not change the public source-snapshot construction
/// contract. The value is written by the audio path and read by UI paths only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompanionActivitySnapshot {
    /// Stable identifier for the companion registration.
    pub(crate) id: u64,
    /// Whether the companion's pre-fader input envelope is above its gate.
    pub(crate) input_active: bool,
}

struct CompanionSlot {
    id: u64,
    fallback_name: String,
    fallback_color: Rgb,
    metadata: Mutex<CompanionMetadata>,
    active: AtomicBool,
    input_active: AtomicBool,
    analysis_interest: AtomicU32,
    mailbox: Arc<SpectrumMailbox>,
}

#[derive(Default)]
struct CompanionMetadata {
    name: Option<String>,
    color: Option<Rgb>,
}

impl CompanionSlot {
    fn display_metadata(&self) -> (String, Rgb) {
        let Ok(metadata) = self.metadata.lock() else {
            return (self.fallback_name.clone(), self.fallback_color);
        };
        let name = metadata
            .name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| self.fallback_name.clone());
        let color = metadata.color.unwrap_or(self.fallback_color);
        (name, color)
    }

    #[cfg(any(feature = "vst3", test))]
    fn set_host_name(&self, name: Option<String>) {
        if let Ok(mut metadata) = self.metadata.lock() {
            metadata.name = name.filter(|name| !name.trim().is_empty());
        }
    }

    #[cfg(any(feature = "vst3", test))]
    fn set_host_color(&self, color: Option<Rgb>) {
        if let Ok(mut metadata) = self.metadata.lock() {
            metadata.color = color;
        }
    }

    fn analysis_requested(&self) -> bool {
        self.analysis_interest.load(Ordering::Acquire) != 0
    }

    fn set_analysis_interest(&self, requested: bool) {
        if requested {
            self.analysis_interest
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    Some(count.saturating_add(1))
                })
                .ok();
        } else {
            self.analysis_interest
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    Some(count.saturating_sub(1))
                })
                .ok();
        }
    }
}

/// Audio-side registration handle retained by a companion processor instance.
pub struct CompanionHandle {
    slot: Arc<CompanionSlot>,
}

impl CompanionHandle {
    /// Return this companion's stable registry identifier.
    pub fn id(&self) -> u64 {
        self.slot.id
    }

    /// Return this companion's stable generated fallback color.
    pub fn color(&self) -> Rgb {
        self.slot.fallback_color
    }

    /// Return the lock-free mailbox owned by this companion.
    pub fn mailbox(&self) -> &SpectrumMailbox {
        self.slot.mailbox.as_ref()
    }

    /// Publish one analyzer frame through this companion's mailbox when it is
    /// requested by a viewer. Returns zero when no viewer is interested.
    pub fn publish(&self, values: &[f32; MAX_BANDS]) -> u64 {
        self.publish_frame(&SpectrumFrame {
            sequence: 0,
            sample_position: None,
            values: *values,
        })
    }

    /// Publish one timed analyzer frame through this companion's mailbox when
    /// it is requested by a viewer.
    pub fn publish_frame(&self, frame: &SpectrumFrame) -> u64 {
        if self.analysis_requested() {
            self.slot.mailbox.publish_frame(frame)
        } else {
            0
        }
    }

    /// Mark the companion active or inactive without touching the registry lock.
    pub fn set_active(&self, active: bool) {
        self.slot.active.store(active, Ordering::Release);
        if !active {
            self.slot.input_active.store(false, Ordering::Release);
        }
    }

    /// Update the optional host track name from a controller-side callback.
    /// This is never called by the audio processor callback.
    #[cfg(any(feature = "vst3", test))]
    pub(crate) fn set_host_name(&self, name: Option<String>) {
        self.slot.set_host_name(name);
    }

    /// Update the optional host track color from a controller-side callback.
    /// This is never called by the audio processor callback.
    #[cfg(any(feature = "vst3", test))]
    pub(crate) fn set_host_color(&self, color: Option<Rgb>) {
        self.slot.set_host_color(color);
    }

    /// Return the current active flag.
    pub fn is_active(&self) -> bool {
        self.slot.active.load(Ordering::Acquire)
    }

    /// Return whether at least one viewer currently requests this source.
    pub fn analysis_requested(&self) -> bool {
        self.slot.analysis_requested()
    }

    /// Publish the companion's pre-fader input-activity gate without locking.
    ///
    /// This is intended for the audio callback. It does not represent channel
    /// volume, mute state, or audibility in the final mix.
    #[cfg(any(feature = "vst3", test))]
    pub(crate) fn set_input_active(&self, input_active: bool) {
        self.slot
            .input_active
            .store(input_active, Ordering::Release);
    }
}

impl Drop for CompanionHandle {
    fn drop(&mut self) {
        self.set_active(false);
    }
}

fn companion_registry() -> &'static Mutex<Vec<Weak<CompanionSlot>>> {
    static REGISTRY: OnceLock<Mutex<Vec<Weak<CompanionSlot>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::with_capacity(MAX_COMPANIONS)))
}

static NEXT_COMPANION_ID: AtomicU64 = AtomicU64::new(1);
static COLOR_COUNTER: AtomicU64 = AtomicU64::new(0xD1B5_4A32_8C7E_0193);

/// Register one companion, returning `None` only when the fixed registry is full.
pub fn register_companion() -> Option<CompanionHandle> {
    let id = NEXT_COMPANION_ID.fetch_add(1, Ordering::Relaxed);
    let time_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    let counter = COLOR_COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    let color = color_from_seed(time_seed ^ counter ^ id.rotate_left(17));
    let slot = Arc::new(CompanionSlot {
        id,
        fallback_name: format!("COMPANION {id}"),
        fallback_color: color,
        metadata: Mutex::new(CompanionMetadata::default()),
        active: AtomicBool::new(true),
        input_active: AtomicBool::new(false),
        analysis_interest: AtomicU32::new(0),
        mailbox: Arc::new(SpectrumMailbox::new()),
    });

    let mut registry = companion_registry().lock().ok()?;
    registry.retain(|entry| entry.strong_count() > 0);
    if registry.len() >= MAX_COMPANIONS {
        return None;
    }
    registry.push(Arc::downgrade(&slot));
    Some(CompanionHandle { slot })
}

/// Change one viewer's analysis interest in a registered companion.
///
/// This is a control/UI operation and may briefly lock the registry. The
/// companion audio callback observes the resulting reference count through an
/// atomic load and never takes this lock. Each viewer must balance a `true`
/// request with one `false` release, including when its editor closes.
pub fn set_companion_analysis_interest(id: u64, requested: bool) -> bool {
    let Ok(registry) = companion_registry().lock() else {
        return false;
    };
    let Some(slot) = registry
        .iter()
        .filter_map(Weak::upgrade)
        .find(|slot| slot.id == id)
    else {
        return false;
    };
    slot.set_analysis_interest(requested);
    true
}

/// Snapshot every registered companion, including inactive entries that have
/// not yet been fully released by the host.
pub fn snapshot_companions() -> Vec<CompanionSourceSnapshot> {
    snapshot_companions_at(None)
}

/// Snapshot every companion, selecting frames near an optional common sample
/// position when one is available.
pub fn snapshot_companions_at(target_sample: Option<i64>) -> Vec<CompanionSourceSnapshot> {
    let Ok(mut registry) = companion_registry().lock() else {
        return Vec::new();
    };
    registry.retain(|entry| entry.strong_count() > 0);
    registry
        .iter()
        .filter_map(Weak::upgrade)
        .map(|slot| {
            let active = slot.active.load(Ordering::Acquire);
            let analysis_requested = slot.analysis_requested();
            let (name, color) = slot.display_metadata();
            let latest_frame = (active && analysis_requested)
                .then(|| slot.mailbox.read())
                .flatten();
            let frame = match target_sample {
                Some(target) => slot
                    .mailbox
                    .read_nearest(target, ALIGNMENT_MAX_SKEW_SAMPLES)
                    // A frame without host timing uses the documented
                    // latest-frame fallback. Timed frames outside the bound
                    // are stale and remain unavailable.
                    .or_else(|| latest_frame.filter(|frame| frame.sample_position.is_none())),
                None => latest_frame,
            };
            CompanionSourceSnapshot {
                id: slot.id,
                name,
                color,
                active,
                analysis_requested,
                frame,
            }
        })
        .collect()
}

/// Snapshot the pre-fader input-activity state for every registered companion.
///
/// This is a UI/control-plane operation. The audio callback only stores the
/// state in an atomic and never takes the registry lock.
pub(crate) fn snapshot_companion_activity() -> Vec<CompanionActivitySnapshot> {
    let Ok(mut registry) = companion_registry().lock() else {
        return Vec::new();
    };
    registry.retain(|entry| entry.strong_count() > 0);
    registry
        .iter()
        .filter_map(Weak::upgrade)
        .map(|slot| CompanionActivitySnapshot {
            id: slot.id,
            input_active: slot.active.load(Ordering::Acquire)
                && slot.input_active.load(Ordering::Acquire),
        })
        .collect()
}

/// Produce a bright deterministic color from a seed for testable color policy.
pub fn color_from_seed(seed: u64) -> Rgb {
    let mixed = split_mix64(seed);
    let hue = ((mixed >> 16) & 0xFFFF) as f32 / 65_535.0;
    hsl_to_rgb(hue, 0.78, 0.60)
}

fn split_mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> Rgb {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue * 6.0;
    let x = chroma * (1.0 - ((sector % 2.0) - 1.0).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let match_value = lightness - chroma / 2.0;
    Rgb::new(
        ((red + match_value) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((green + match_value) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((blue + match_value) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timed_frame(sequence: u64, sample_position: i64, value: f32) -> SpectrumFrame {
        SpectrumFrame {
            sequence,
            sample_position: Some(sample_position),
            values: [value; MAX_BANDS],
        }
    }

    #[test]
    fn mailbox_is_empty_until_a_frame_is_published() {
        let mailbox = SpectrumMailbox::new();
        assert!(mailbox.read().is_none());
        let values = [1.5; MAX_BANDS];
        let sequence = mailbox.publish(&values);
        let frame = mailbox.read().expect("published frame should be readable");
        assert_eq!(frame.sequence, sequence);
        assert_eq!(frame.values, values);
    }

    #[test]
    fn mailbox_selects_the_nearest_timed_frame_and_rejects_stale_data() {
        let mailbox = SpectrumMailbox::new();
        mailbox.publish_frame(&timed_frame(1, 1_000, 1.0));
        mailbox.publish_frame(&timed_frame(2, 3_000, 2.0));
        mailbox.publish_frame(&timed_frame(3, 5_000, 3.0));

        let nearest = mailbox
            .read_nearest(2_900, 200)
            .expect("nearby timed frame should be selected");
        assert_eq!(nearest.sample_position, Some(3_000));
        assert_eq!(nearest.values, [2.0; MAX_BANDS]);
        assert!(mailbox.read_nearest(8_000, 1_000).is_none());
    }

    #[test]
    fn mailbox_history_has_a_fixed_depth_and_expires_old_frames() {
        let mailbox = SpectrumMailbox::new();
        for index in 0..(FRAME_HISTORY_LEN + 2) {
            mailbox.publish_frame(&timed_frame(
                index as u64,
                index as i64 * 1_000,
                index as f32,
            ));
        }

        assert!(mailbox.read_nearest(0, 0).is_none());
        let newest_position = (FRAME_HISTORY_LEN + 1) as i64 * 1_000;
        assert_eq!(
            mailbox
                .read_nearest(newest_position, 0)
                .expect("newest history entry should remain")
                .sample_position,
            Some(newest_position)
        );
    }

    #[test]
    fn seeded_companion_colors_are_stable_and_distinct_for_adjacent_seeds() {
        assert_eq!(color_from_seed(17), color_from_seed(17));
        assert_ne!(color_from_seed(17), color_from_seed(18));
    }

    #[test]
    fn inactive_companion_remains_visible_until_its_handle_is_released() {
        let handle = register_companion().expect("test registry should have capacity");
        let id = handle.id();
        handle.set_active(false);
        let inactive = snapshot_companions()
            .into_iter()
            .find(|source| source.id == id)
            .expect("inactive source should remain discoverable");
        assert!(!inactive.active);
        assert!(inactive.frame.is_none());
        drop(handle);
        assert!(!snapshot_companions().iter().any(|source| source.id == id));
    }

    #[test]
    fn host_metadata_replaces_and_falls_back_to_generated_identity() {
        let handle = register_companion().expect("test registry should have capacity");
        let id = handle.id();
        let fallback_name = format!("COMPANION {id}");
        let fallback_color = handle.color();
        let host_color = Rgb::new(17, 129, 241);

        handle.set_host_name(Some("Bass Bus".to_string()));
        handle.set_host_color(Some(host_color));
        let host_source = snapshot_companions()
            .into_iter()
            .find(|source| source.id == id)
            .expect("host metadata source should be discoverable");
        assert_eq!(host_source.name, "Bass Bus");
        assert_eq!(host_source.color, host_color);

        handle.set_host_name(None);
        handle.set_host_color(None);
        let fallback_source = snapshot_companions()
            .into_iter()
            .find(|source| source.id == id)
            .expect("fallback source should remain discoverable");
        assert_eq!(fallback_source.name, fallback_name);
        assert_eq!(fallback_source.color, fallback_color);
    }

    #[test]
    fn companion_interest_is_reference_counted_and_unrequested_sources_have_no_frame() {
        let handle = register_companion().expect("test registry should have capacity");
        let id = handle.id();
        let values = [1.0; MAX_BANDS];
        let _ = handle.publish(&values);

        let idle = snapshot_companions()
            .into_iter()
            .find(|source| source.id == id)
            .expect("idle source should remain discoverable");
        assert!(!idle.analysis_requested);
        assert!(idle.frame.is_none());

        assert!(set_companion_analysis_interest(id, true));
        assert!(set_companion_analysis_interest(id, true));
        assert!(handle.analysis_requested());
        assert_ne!(handle.publish(&values), 0);
        let requested = snapshot_companions()
            .into_iter()
            .find(|source| source.id == id)
            .expect("requested source should remain discoverable");
        assert!(requested.frame.is_some());
        assert!(set_companion_analysis_interest(id, false));
        assert!(handle.analysis_requested());
        assert!(set_companion_analysis_interest(id, false));
        assert!(!handle.analysis_requested());
        assert!(set_companion_analysis_interest(id, false));
        assert!(!handle.analysis_requested());
    }

    #[test]
    fn companion_input_activity_is_atomic_and_visible_without_analysis_interest() {
        let handle = register_companion().expect("test registry should have capacity");
        let id = handle.id();

        assert!(
            !snapshot_companion_activity()
                .into_iter()
                .find(|source| source.id == id)
                .expect("source activity should be discoverable")
                .input_active
        );
        handle.set_input_active(true);
        assert!(
            snapshot_companion_activity()
                .into_iter()
                .find(|source| source.id == id)
                .expect("source activity should remain discoverable")
                .input_active
        );

        handle.set_active(false);
        assert!(
            !snapshot_companion_activity()
                .into_iter()
                .find(|source| source.id == id)
                .expect("inactive source should remain discoverable")
                .input_active
        );
    }

    #[test]
    fn registry_capacity_contract_is_at_least_128_sources() {
        const { assert!(MAX_COMPANIONS >= 128) };
    }
}
