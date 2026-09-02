//! Realtime-safe VST3 audio processors for the viewer and companion classes.

use std::cell::UnsafeCell;
use std::sync::Arc;

use toybox::vst3::prelude::Steinberg::*;
use toybox::vst3::prelude::*;

use crate::activity::InputActivityMeter;
use crate::analysis::SpectrumAnalyzer;
use crate::instance_registry::{SharedRole, release_shared_for_role};
use crate::shared::{ChromascopeShared, DeviceKind};

use super::{COMPANION_CONTROLLER_CID, VIEWER_CONTROLLER_CID, vst3_bus_flag as vst3_flag};

/// One stereo processor paired with a shared viewer or companion runtime.
pub(super) struct ChromascopeVst3Processor {
    kind: DeviceKind,
    shared: Arc<ChromascopeShared>,
    analyzer: UnsafeCell<SpectrumAnalyzer>,
    analysis_requested: UnsafeCell<bool>,
    activity_meter: UnsafeCell<InputActivityMeter>,
    sample_rate_hz: UnsafeCell<f32>,
}

// VST3 calls the processor through a shared COM reference, while the host
// serializes setup/activation with process callbacks. The analyzer is only
// mutated by that one processor's lifecycle and process path.
unsafe impl Send for ChromascopeVst3Processor {}
unsafe impl Sync for ChromascopeVst3Processor {}

impl ChromascopeVst3Processor {
    /// Create a processor with all FFT storage allocated before audio starts.
    pub(super) fn new(kind: DeviceKind, shared: Arc<ChromascopeShared>) -> Self {
        Self {
            kind,
            shared,
            analyzer: UnsafeCell::new(SpectrumAnalyzer::new(48_000.0)),
            analysis_requested: UnsafeCell::new(kind == DeviceKind::Viewer),
            activity_meter: UnsafeCell::new(InputActivityMeter::default()),
            sample_rate_hz: UnsafeCell::new(48_000.0),
        }
    }

    fn should_analyze(&self) -> bool {
        match self.kind {
            DeviceKind::Viewer => true,
            DeviceKind::Companion => self.shared.companion_analysis_requested(),
        }
    }

    /// Fail closed when the host supplies a process block we cannot consume.
    ///
    /// This stays on the callback's lock-free path: the meter is local to this
    /// processor and `set_input_active` stores directly into the companion's
    /// atomic mailbox. A rejected block must not leave a stale activity dot
    /// visible indefinitely.
    #[inline]
    fn reset_input_activity(&self) {
        unsafe {
            (&mut *self.activity_meter.get()).reset();
        }
        self.shared.set_input_active(false);
    }
}

impl Drop for ChromascopeVst3Processor {
    fn drop(&mut self) {
        self.shared.set_active(false);
        release_shared_for_role(&self.shared, SharedRole::Processor);
    }
}

impl Class for ChromascopeVst3Processor {
    type Interfaces = (IComponent, IAudioProcessor, IProcessContextRequirements);
}

impl IPluginBaseTrait for ChromascopeVst3Processor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for ChromascopeVst3Processor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        if class_id.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            *class_id = match self.kind {
                DeviceKind::Viewer => VIEWER_CONTROLLER_CID,
                DeviceKind::Companion => COMPANION_CONTROLLER_CID,
            };
        }
        kResultOk
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media_type: MediaType, dir: BusDirection) -> i32 {
        match media_type as MediaTypes {
            MediaTypes_::kAudio => match dir as BusDirections {
                BusDirections_::kInput | BusDirections_::kOutput => 1,
                _ => 0,
            },
            _ => 0,
        }
    }

    unsafe fn getBusInfo(
        &self,
        media_type: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if bus.is_null() || index != 0 || media_type as MediaTypes != MediaTypes_::kAudio {
            return kInvalidArgument;
        }
        let label = match dir as BusDirections {
            BusDirections_::kInput => {
                if self.kind == DeviceKind::Companion {
                    "Companion Input"
                } else {
                    "Main Input"
                }
            }
            BusDirections_::kOutput => "Output",
            _ => return kInvalidArgument,
        };
        let bus = unsafe { &mut *bus };
        bus.mediaType = MediaTypes_::kAudio as MediaType;
        bus.direction = dir;
        bus.channelCount = 2;
        copy_wstring(label, &mut bus.name);
        bus.busType = BusTypes_::kMain as BusType;
        bus.flags = vst3_flag(BusInfo_::BusFlags_::kDefaultActive);
        kResultOk
    }

    unsafe fn getRoutingInfo(
        &self,
        _in_info: *mut RoutingInfo,
        _out_info: *mut RoutingInfo,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        media_type: MediaType,
        dir: BusDirection,
        index: i32,
        _state: TBool,
    ) -> tresult {
        if media_type as MediaTypes != MediaTypes_::kAudio
            || index != 0
            || !matches!(
                dir as BusDirections,
                BusDirections_::kInput | BusDirections_::kOutput
            )
        {
            return kInvalidArgument;
        }
        kResultOk
    }

    unsafe fn setActive(&self, state: TBool) -> tresult {
        self.shared.set_active(state != 0);
        kResultOk
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
}

impl IAudioProcessorTrait for ChromascopeVst3Processor {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut SpeakerArrangement,
        num_ins: i32,
        outputs: *mut SpeakerArrangement,
        num_outs: i32,
    ) -> tresult {
        if num_ins != 1 || num_outs != 1 || inputs.is_null() || outputs.is_null() {
            return kInvalidArgument;
        }
        if unsafe { *inputs } != SpeakerArr::kStereo || unsafe { *outputs } != SpeakerArr::kStereo {
            return kResultFalse;
        }
        kResultTrue
    }

    unsafe fn getBusArrangement(
        &self,
        dir: BusDirection,
        index: i32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        if arr.is_null() || index != 0 {
            return kInvalidArgument;
        }
        if matches!(
            dir as BusDirections,
            BusDirections_::kInput | BusDirections_::kOutput
        ) {
            unsafe {
                *arr = SpeakerArr::kStereo;
            }
            kResultOk
        } else {
            kInvalidArgument
        }
    }

    unsafe fn canProcessSampleSize(&self, symbolic_sample_size: i32) -> tresult {
        match symbolic_sample_size as SymbolicSampleSizes {
            SymbolicSampleSizes_::kSample32 => kResultOk,
            SymbolicSampleSizes_::kSample64 => kNotImplemented,
            _ => kInvalidArgument,
        }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        if let Some(setup) = unsafe { setup.as_ref() } {
            // The host calls setupProcessing outside the realtime process path.
            unsafe {
                (&mut *self.analyzer.get()).set_sample_rate_hz(setup.sampleRate as f32);
                *self.sample_rate_hz.get() = setup.sampleRate as f32;
            }
        }
        kResultOk
    }

    unsafe fn setProcessing(&self, state: TBool) -> tresult {
        self.shared.set_active(state != 0);
        if state == 0 {
            unsafe {
                (&mut *self.activity_meter.get()).reset();
            }
        }
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let Some(data) = (unsafe { data.as_ref() }) else {
            self.reset_input_activity();
            return kInvalidArgument;
        };
        if data.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as i32 {
            self.reset_input_activity();
            return process_ok();
        }

        let Some(buffers) = (unsafe { stereo_f32_buffers(data) }) else {
            self.reset_input_activity();
            return process_ok();
        };
        let block_start_sample = process_context_sample_position(data);
        let published = {
            let analyzer = unsafe { &mut *self.analyzer.get() };
            let requested = self.should_analyze();
            let was_requested = unsafe { &mut *self.analysis_requested.get() };
            if requested != *was_requested {
                analyzer.reset();
            }
            *was_requested = requested;
            if requested
                && analyzer.process_stereo_block_at(
                    buffers.input_left,
                    buffers.input_right,
                    block_start_sample,
                )
            {
                analyzer.latest_frame()
            } else {
                None
            }
        };
        if let Some(frame) = published {
            self.shared.publish_frame(&frame);
        }

        let track_input_activity = self.kind == DeviceKind::Companion;
        let mut input_peak: f32 = 0.0;
        for sample_index in 0..buffers.num_samples {
            let input_left = buffers.input_left[sample_index];
            let input_right = buffers.input_right[sample_index];
            buffers.output_left[sample_index] = input_left;
            buffers.output_right[sample_index] = input_right;
            if track_input_activity {
                input_peak = input_peak
                    .max(finite_abs_sample(input_left))
                    .max(finite_abs_sample(input_right));
            }
        }
        if track_input_activity {
            let input_active = unsafe {
                (&mut *self.activity_meter.get()).process_block(
                    input_peak,
                    buffers.num_samples,
                    *self.sample_rate_hz.get(),
                )
            };
            self.shared.set_input_active(input_active);
        }
        process_ok()
    }

    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

impl IProcessContextRequirementsTrait for ChromascopeVst3Processor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        vst3_flag(IProcessContextRequirements_::Flags_::kNeedContinousTimeSamples)
    }
}

/// Read the host's common sample timeline for one process block.
///
/// Continuous time is preferred because it does not jump at loop boundaries.
/// The VST3 contract defines `projectTimeSamples` as always valid, so it is a
/// safe fallback when the optional continuous-time validity flag is absent.
/// A missing process context is explicitly untimed.
fn process_context_sample_position(data: &ProcessData) -> Option<i64> {
    let context = unsafe { data.processContext.as_ref() }?;
    if context.state & vst3_flag(ProcessContext_::StatesAndFlags_::kContTimeValid) != 0 {
        Some(context.continousTimeSamples)
    } else {
        Some(context.projectTimeSamples)
    }
}

/// Return a finite absolute sample value for the companion's pre-fader
/// activity indicator. Peak accumulation happens in the passthrough loop so
/// the input buffers are traversed only once per callback.
#[inline]
fn finite_abs_sample(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.abs()
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_processor_only_requests_fft_work_when_a_viewer_is_interested() {
        let shared = Arc::new(ChromascopeShared::new(DeviceKind::Companion));
        let processor = ChromascopeVst3Processor::new(DeviceKind::Companion, shared.clone());

        assert!(!processor.should_analyze());
        let id = shared
            .companion_id()
            .expect("test companion should have a registry id");
        assert!(crate::registry::set_companion_analysis_interest(id, true));
        assert!(processor.should_analyze());
        assert!(crate::registry::set_companion_analysis_interest(id, false));
        assert!(!processor.should_analyze());
    }

    #[test]
    fn processor_requests_the_continuous_sample_timeline() {
        assert_eq!(
            unsafe {
                ChromascopeVst3Processor::new(
                    DeviceKind::Viewer,
                    Arc::new(ChromascopeShared::new(DeviceKind::Viewer)),
                )
                .getProcessContextRequirements()
            },
            vst3_flag(IProcessContextRequirements_::Flags_::kNeedContinousTimeSamples)
        );
    }

    #[test]
    fn process_context_position_prefers_continuous_and_falls_back_to_project_time() {
        let mut project_context: ProcessContext = unsafe { std::mem::zeroed() };
        project_context.projectTimeSamples = 1_000;
        project_context.continousTimeSamples = 2_000;
        let mut project_data: ProcessData = unsafe { std::mem::zeroed() };
        project_data.processContext = &mut project_context;
        assert_eq!(process_context_sample_position(&project_data), Some(1_000));

        let mut continuous_context: ProcessContext = unsafe { std::mem::zeroed() };
        continuous_context.projectTimeSamples = 1_000;
        continuous_context.continousTimeSamples = 2_000;
        continuous_context.state = vst3_flag(ProcessContext_::StatesAndFlags_::kContTimeValid);
        let mut continuous_data: ProcessData = unsafe { std::mem::zeroed() };
        continuous_data.processContext = &mut continuous_context;
        assert_eq!(
            process_context_sample_position(&continuous_data),
            Some(2_000)
        );

        let empty_data: ProcessData = unsafe { std::mem::zeroed() };
        assert_eq!(process_context_sample_position(&empty_data), None);
    }

    #[test]
    fn companion_activity_peak_ignores_non_finite_samples() {
        assert_eq!(finite_abs_sample(f32::NAN), 0.0);
        assert_eq!(finite_abs_sample(f32::INFINITY), 0.0);
        assert_eq!(finite_abs_sample(-0.25), 0.25);
    }

    struct StereoProcessFixture {
        process_data: ProcessData,
        _input_left: Vec<f32>,
        _input_right: Vec<f32>,
        _output_left: Vec<f32>,
        _output_right: Vec<f32>,
        _input_channel_buffers: Vec<*mut f32>,
        _output_channel_buffers: Vec<*mut f32>,
        _input_buses: Vec<AudioBusBuffers>,
        _output_buses: Vec<AudioBusBuffers>,
    }

    impl StereoProcessFixture {
        fn with_input(samples: usize, value: f32) -> Self {
            let mut input_left = vec![value; samples];
            let mut input_right = vec![value; samples];
            let mut output_left = vec![0.0; samples];
            let mut output_right = vec![0.0; samples];
            let mut input_channel_buffers = vec![input_left.as_mut_ptr(), input_right.as_mut_ptr()];
            let mut output_channel_buffers =
                vec![output_left.as_mut_ptr(), output_right.as_mut_ptr()];
            let input_bus = AudioBusBuffers {
                numChannels: 2,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: input_channel_buffers.as_mut_ptr(),
                },
            };
            let output_bus = AudioBusBuffers {
                numChannels: 2,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: output_channel_buffers.as_mut_ptr(),
                },
            };
            let mut input_buses = vec![input_bus];
            let mut output_buses = vec![output_bus];
            let mut process_data: ProcessData = unsafe { std::mem::zeroed() };
            process_data.symbolicSampleSize = SymbolicSampleSizes_::kSample32 as i32;
            process_data.numInputs = 1;
            process_data.numOutputs = 1;
            process_data.numSamples = i32::try_from(samples).expect("sample count must fit i32");
            process_data.inputs = input_buses.as_mut_ptr();
            process_data.outputs = output_buses.as_mut_ptr();

            Self {
                process_data,
                _input_left: input_left,
                _input_right: input_right,
                _output_left: output_left,
                _output_right: output_right,
                _input_channel_buffers: input_channel_buffers,
                _output_channel_buffers: output_channel_buffers,
                _input_buses: input_buses,
                _output_buses: output_buses,
            }
        }
    }

    fn assert_rejected_process_clears_companion_activity(
        processor: &ChromascopeVst3Processor,
        shared: &ChromascopeShared,
        rejected_data: *mut ProcessData,
        expected_result: tresult,
    ) {
        let mut valid = StereoProcessFixture::with_input(4_800, 1.0);
        assert_eq!(
            unsafe {
                <ChromascopeVst3Processor as IAudioProcessorTrait>::process(
                    processor,
                    &mut valid.process_data,
                )
            },
            kResultOk
        );
        let id = shared
            .companion_id()
            .expect("test companion should have a registry id");
        assert!(
            crate::registry::snapshot_companion_activity()
                .into_iter()
                .find(|activity| activity.id == id)
                .expect("active companion should have an activity snapshot")
                .input_active
        );

        assert_eq!(
            unsafe {
                <ChromascopeVst3Processor as IAudioProcessorTrait>::process(
                    processor,
                    rejected_data,
                )
            },
            expected_result
        );
        assert!(
            !crate::registry::snapshot_companion_activity()
                .into_iter()
                .find(|activity| activity.id == id)
                .expect("companion should remain discoverable after rejection")
                .input_active
        );
    }

    #[test]
    fn null_process_data_fails_closed_for_companion_activity() {
        let shared = Arc::new(ChromascopeShared::new(DeviceKind::Companion));
        let processor = ChromascopeVst3Processor::new(DeviceKind::Companion, shared.clone());

        assert_rejected_process_clears_companion_activity(
            &processor,
            &shared,
            std::ptr::null_mut(),
            kInvalidArgument,
        );
    }

    #[test]
    fn unsupported_sample_size_fails_closed_for_companion_activity() {
        let shared = Arc::new(ChromascopeShared::new(DeviceKind::Companion));
        let processor = ChromascopeVst3Processor::new(DeviceKind::Companion, shared.clone());
        let mut rejected_data: ProcessData = unsafe { std::mem::zeroed() };
        rejected_data.symbolicSampleSize = SymbolicSampleSizes_::kSample64 as i32;

        assert_rejected_process_clears_companion_activity(
            &processor,
            &shared,
            &mut rejected_data,
            kResultOk,
        );
    }

    #[test]
    fn invalid_stereo_buffers_fail_closed_for_companion_activity() {
        let shared = Arc::new(ChromascopeShared::new(DeviceKind::Companion));
        let processor = ChromascopeVst3Processor::new(DeviceKind::Companion, shared.clone());
        let mut rejected_data: ProcessData = unsafe { std::mem::zeroed() };
        rejected_data.symbolicSampleSize = SymbolicSampleSizes_::kSample32 as i32;

        assert_rejected_process_clears_companion_activity(
            &processor,
            &shared,
            &mut rejected_data,
            kResultOk,
        );
    }
}
