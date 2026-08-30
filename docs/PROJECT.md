# Project

## Summary

Chromascope is a small Toybox VST3 experiment for replacing unreliable
selected-track detection with explicit companion devices. One bundle exposes
the Chromascope viewer and Chromascope Companion analyzer classes. The viewer
always analyzes its own stereo input and can overlay selected spectra from up
to 128 concurrently registered companions.

The companion registry is deliberately explicit and process-local. A
companion processor registers one of 128 bounded source slots, publishes FFT
frames through its lock-free mailbox only while at least one viewer selects it,
and keeps its generated color until its shared runtime is released. The viewer
snapshots that registry on the UI path and never asks the DAW which track is
selected or enumerates host tracks. Registered but unselected companions stay
discoverable as `IDLE` and pass audio without running the FFT.

## Constraints

- Keep DSP realtime-safe: no allocation or blocking lock in the audio callback.
- Keep cross-instance exchange bounded: fixed-size FFT frames and atomic
  sequence/value storage only.
- Handle source warming, inactive processors, registry capacity, and host
  teardown without dereferencing unavailable source data.
- Keep the repo thin: framework/GUI mechanics belong in Toybox.

## Analyzer semantics

- The default editor is a 1440 x 640 (2.25:1) wide scope. Both editor paths use
  a logarithmic 20 Hz to 20 kHz frequency axis and line traces. The native
  source browser clips and scrolls a virtualized visible-row slice so all 128
  source slots remain selectable without making the panel tall.
- Each processor uses an 8192-sample Hann-windowed FFT with a 2048-sample hop.
  Left and right channels are transformed independently; per-band power is the
  arithmetic mean of the two one-sided channel powers, so an out-of-phase
  stereo signal cannot disappear in a mono fold.
- The plotted value is the normalized spectral power converted with
  `10 * log10(power)`, then clamped to the display range. A display-only tilt
  of `+4.5 dB * log2(frequency / 1000 Hz)` is applied after conversion: 1 kHz
  is unchanged, each octave above it rises 4.5 dB, and each octave below it
  falls 4.5 dB. Raw values at or below the -100 dBFS display floor are gated to
  a flat -100 dB and do not receive tilt; values above the floor retain the
  tilt. This prevents silence from becoming a diagonal while preserving real
  low-level signals. This never changes the audio passthrough or mailbox input.
- Published display values use dB-domain exponential ballistics: 45 ms attack
  and 280 ms release, with one update per 2048-sample hop (about 23 Hz at
  48 kHz). The native editor applies a second UI-only 20 ms attack / 100 ms
  release stage to both main and companion traces, using the elapsed repaint
  interval capped at 50 ms. Its preferred presentation cadence is 60 Hz when
  the host/editor can provide it; the pinned generic Toybox macOS bridge's
  fallback timer is currently about 30 Hz. No extra FFT or audio latency is
  introduced.
- Each 64-band frame is resampled to 256 display points with clamped cubic
  Hermite interpolation in logarithmic-band space. This removes visible
  segment stepping and lets the Radiant/Vello stroke renderer anti-alias the
  curve; it is display smoothing only and does not add spectral resolution or
  change mailbox data.

## Visual system

Chromascope mirrors Pump's canonical dark-coral visual system in
`src/visual_system.rs`: `#1B1E1E` charcoal canvas/surfaces, `#2A2D2D` overlay,
`#3A3D3D` border, `#404342` emphasized border, `#363939` strong grid,
`#282B2B` soft grid, `#D8D7D3` primary text, and `#999B9A` muted text. The
viewer main trace uses Pump's primary coral `#E95843`; Pump's secondary coral
`#F16C56` is used for live/selected UI emphasis and `#D9975F` for warming
state. Companion traces retain their stable source-specific colors so several
selected sources remain unmistakable against the Pump surface.

The native Radiant editor follows Pump's compact 3.4/6.8/10.2/13.6 spacing
roles, 6.8 px rounded surface radius, one-pixel borders, and mono typography
hierarchy. The declarative fallback receives the same semantic palette through
Pump-aligned `ThemeTokens`; integer Patchbay spacing is rounded from the native
logical values.

Source activation and visual highlighting are separate UI states. A normal
source-row click changes analyzer interest and therefore controls whether that
companion contributes a trace. A Command-click changes only the persistent
highlight set. At most eight active sources can be highlighted at once; each
entry owns one stable near-white tint, and that exact tint is used for both the
source row and the corresponding scope trace. Attempting a ninth highlight
keeps all existing entries and reports that the set is full. Highlights are
removed when their companion becomes inactive or leaves the registry and never
add work to the audio callback.

## Analysis-frame alignment

When a VST3 process context is present, each published frame is stamped with
the exclusive end sample of its analysis window. Continuous sample time is
preferred because it remains stable through loop boundaries; project sample
time is used when continuous-time validity is absent. A null context produces
an untimed frame and activates the explicit latest-frame fallback.

Every mailbox keeps eight frames in fixed atomic slots. The viewer anchors to
its latest timed main frame, considers only selected active/requested
companions within `2 * FFT_HOP_SIZE` samples, and chooses the oldest eligible
sample as the common target. Each mailbox then chooses its nearest frame to that
target, preferring an at-or-before frame on a distance tie. Timed frames with
no nearby history are hidden as unavailable rather than displaying stale data.
This corrects cross-instance callback/window scheduling skew only; it does not
compensate actual plugin or routing latency.

## Ableton track metadata

The companion controller implements the optional VST3 `ChannelContext::IInfoListener`
interface. When Ableton Live supplies `kChannelNameKey` and
`kChannelColorKey`, the viewer uses the live track name and ARGB color for the
source row and trace. Missing or empty fields safely fall back to
`COMPANION <id>` and the generated registration color. The metadata mutex is
used only by controller/UI registry paths; audio callbacks never acquire it.

This is the supported Ableton-oriented path, not selected-track discovery or
host enumeration. Hosts that do not send the optional context callback retain
the fallback identity.

## Companion input activity

The source browser's blinking red dot is a bounded diagnostic of pre-fader
input activity at each companion device. The companion audio callback computes
one finite stereo block peak, applies a 5 ms attack and 100 ms release envelope,
and uses -60 dBFS on / -66 dBFS off hysteresis before publishing one atomic
boolean to the process-local registry. The UI reads that flag and blinks the
dot at a 0.8 second period. This indicator is deliberately not labeled as
channel volume, mute state, loudness, or final audible activity; the current
companion feed is before the host channel fader.

## Prototype boundaries

- VST3 only; CLAP is intentionally not exported by this prototype.
- Each device exposes one stereo input and one stereo passthrough output and
  processes 32-bit samples.
- The registry is process-local. It works when the viewer and companions are
  loaded from this bundle in the same plugin host process; it is not an
  inter-process transport.
- The registry has a documented hard limit of 128 concurrent companion slots.
  The list is scrollable, selection supports the full bounded set, and FFT
  work/publication is demand-driven by the aggregate selection interest from
  open viewers.
- Viewer source selection is UI-lifetime state and is not persisted as a
  parameter. The companion has no editor.
