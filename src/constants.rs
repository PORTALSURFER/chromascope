//! Stable product and realtime-analysis constants.

/// Human-readable name used by the VST3 classes and the editor.
pub const PLUGIN_NAME: &str = "Chromascope";
/// Human-readable name used by the companion VST3 classes.
pub const COMPANION_NAME: &str = "Chromascope Companion";
/// Stable product identifier used by host-facing metadata.
pub const PLUGIN_ID: &str = "com.portalsurfer.chromascope";

/// Default editor width in logical pixels (2.25:1 scope layout).
pub const WINDOW_WIDTH: u32 = 1440;
/// Default editor height in logical pixels (2.25:1 scope layout).
pub const WINDOW_HEIGHT: u32 = 640;

/// FFT size used by both the viewer and companion processors.
pub const FFT_SIZE: usize = 8192;
/// Number of input samples between published FFT frames (FFT / 4 overlap).
pub const FFT_HOP_SIZE: usize = FFT_SIZE / 4;
/// Number of logarithmically spaced spectrum bins shown in the editor.
pub const MAX_BANDS: usize = 64;
/// Maximum number of concurrent companion registrations retained by the process
/// registry. This is a bounded control-plane limit; it is intentionally much
/// larger than the visible source-list viewport and does not make audio work
/// proportional to registration count.
pub const MAX_COMPANIONS: usize = 128;
/// Number of display samples used to draw each spectrum trace.
///
/// The analyzer still publishes [`MAX_BANDS`](self::MAX_BANDS) measured bands.
/// These extra points are display-domain interpolation only and do not add FFT
/// resolution or change the values exchanged between audio instances.
pub const DISPLAY_TRACE_SAMPLES: usize = 256;
/// Number of recent frames retained per mailbox for common-time selection.
///
/// This is a fixed allocation made before audio processing starts. It gives
/// the viewer enough history to select a nearby frame when two plugin
/// instances publish on adjacent host callbacks without making cross-instance
/// exchange unbounded.
pub const FRAME_HISTORY_LEN: usize = 8;
/// Maximum timestamp skew accepted when selecting an aligned frame.
///
/// Two analysis hops cover normal callback scheduling differences while
/// rejecting a genuinely stale source instead of displaying an old frame.
pub const ALIGNMENT_MAX_SKEW_SAMPLES: i64 = (FFT_HOP_SIZE * 2) as i64;
/// Maximum presentation step after a delayed UI repaint.
///
/// The cap bounds a single visible jump; it does not delay audio or alter
/// analysis timestamps.
pub const PRESENTATION_MAX_STEP_SECONDS: f32 = 0.050;
/// Preferred native-editor presentation cadence.
///
/// The retained editor is ready for a 60 Hz host repaint driver. The host may
/// provide a lower cadence; that limitation is kept separate from DSP and the
/// presentation smoother remains elapsed-time based.
pub const PRESENTATION_TARGET_FPS: u32 = 60;
/// Elapsed time represented by one preferred presentation tick.
pub const PRESENTATION_FRAME_SECONDS: f32 = 1.0 / PRESENTATION_TARGET_FPS as f32;
/// Display-domain attack time used to pace movement between analysis frames.
pub const PRESENTATION_ATTACK_SECONDS: f32 = 0.020;
/// Display-domain release time used to keep decay readable without a long
/// additional hold after the analyzer's own release ballistics.
pub const PRESENTATION_RELEASE_SECONDS: f32 = 0.100;
/// Lowest frequency represented by the spectrum graph.
pub const MIN_FREQUENCY_HZ: f32 = 20.0;
/// Highest frequency represented by the spectrum graph.
pub const MAX_FREQUENCY_HZ: f32 = 20_000.0;
/// Lowest displayed level in decibels.
pub const MIN_LEVEL_DB: f32 = -100.0;
/// Highest displayed level in decibels.
pub const MAX_LEVEL_DB: f32 = 3.0;

/// Display-only spectral tilt applied relative to [`SPECTRAL_TILT_REFERENCE_HZ`].
///
/// A positive value lifts higher frequencies by this many dB per octave. It
/// changes only the plotted values; the audio passthrough is never tilted.
pub const SPECTRAL_TILT_DB_PER_OCTAVE: f32 = 4.5;
/// Frequency around which the display-only tilt is zero.
pub const SPECTRAL_TILT_REFERENCE_HZ: f32 = 1_000.0;
/// Exponential attack time for a rising spectrum trace.
pub const SPECTRUM_ATTACK_SECONDS: f32 = 0.045;
/// Exponential release time for a falling spectrum trace.
pub const SPECTRUM_RELEASE_SECONDS: f32 = 0.280;

/// Shared graph heading used by the declarative and native editor paths.
pub const SPECTRUM_HEADER: &str = "SPECTRUM  20 Hz—20 kHz  +4.5 dB/oct VIEW";

/// Fixed Pump-coral color for the viewer's own input spectrum.
///
/// This is Pump's `accent_mint` token (`#E95843`), retained as one stable
/// primary trace while companion traces keep their per-source colors.
pub const MAIN_SPECTRUM_COLOR: Rgb = Rgb::new(233, 88, 67);

/// Small RGB value type shared by the registry and UI renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    /// Red channel.
    pub red: u8,
    /// Green channel.
    pub green: u8,
    /// Blue channel.
    pub blue: u8,
}

impl Rgb {
    /// Construct an RGB color.
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}
