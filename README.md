# Chromascope

Chromascope is a Toybox-based VST3 prototype for viewing one track's spectrum
alongside spectra published by companion devices on other audio tracks.

The bundle contains two host-visible devices:

- Chromascope is the primary viewer. It draws its own input in Pump's fixed
  primary coral and
  lets the user select multiple companion sources for overlaid traces.
- Chromascope Companion is an audio-track analyzer. Each live registration
  receives one stable generated fallback color, used by both its graph trace
  and its source-list marker unless Live supplies its track color. The source
  row also has a small blinking red input-activity dot when the companion's
  pre-fader input exceeds the diagnostic gate; it is not a fader, mute, or
  final-audibility indicator.

Companions are discovered through an explicit process-local registry owned by
this bundle. The prototype does not use DAW selected-track APIs or host track
enumeration. Audio callbacks only touch preallocated FFT state and atomic
mailboxes; registry locks are limited to instance/UI lifecycle paths.

The viewer is a wide 2.25:1 analyzer using an 8192-sample Hann FFT, independent
stereo power analysis, a logarithmic frequency axis, display-only +4.5 dB/oct
tilt around 1 kHz, slower attack/release ballistics, and a dense 256-point
display interpolation of the 64 measured bands. The interpolation is display
smoothing only; it does not claim extra spectral resolution. On Ableton Live,
the companion controller accepts the supported VST3 channel-context name/color
callback; when unavailable, each row and trace keeps its generated fallback
label/color.

The plotted tilt has an explicit floor gate: a raw bin at or below -100 dBFS is
drawn at a flat -100 dB floor and is not tilted. A real signal above that floor
still receives the +4.5 dB/oct display tilt, so silence cannot turn into a
diagonal noise-floor line. Tilt, ballistics, and interpolation affect only the
display; passthrough audio and published measured data are not changed.

Frames carry the exclusive end sample of their 8192-sample analysis window when
the VST3 process context supplies a timeline. Continuous time is preferred,
with project sample time as the documented context fallback; a missing context
is explicitly untimed. Each mailbox retains eight fixed-size frames. The viewer
chooses a common target among the main trace and selected companions within two
2048-sample hops, rejects timed frames outside that bound as stale, and uses the
latest-frame fallback only for untimed hosts. This removes analyzer scheduling
skew without concealing genuine signal-chain latency.

The native editor's presentation layer is ready for a 60 Hz repaint cadence and
uses the same display-domain 20 ms attack / 100 ms release smoother for the
main and companion traces. A delayed repaint is capped at 50 ms per visible
step, so host timer jitter does not create a large jump. This is a UI clock and
does not add audio latency or FFT work. The pinned Toybox macOS bridge currently
drives its generic realtime redraw fallback at about 30 Hz; a host or newer
bridge that supplies 60 Hz repaint callbacks is used directly by the same
elapsed-time contract.

The editor follows Pump's visual system: `#1B1E1E` charcoal surfaces,
`#3A3D3D` borders, `#D8D7D3` primary text, muted gray grid lines, and Pump's
`#E95843` primary coral for the main trace with `#F16C56` for live/status
emphasis. The compact 3.4/6.8/10.2/13.6 spacing roles, 6.8 px surface radius,
and mono typography hierarchy are mirrored in `src/visual_system.rs`. Companion
colors remain stable, source-specific accents layered over this shared neutral
surface so multiple overlays remain distinguishable.

The process-local registry supports 128 concurrent companions. The source
browser remains compact and scrollable, while unselected registered companions
remain discoverable as `IDLE` without running or publishing their FFT. Selecting
a source contributes viewer interest through an atomic reference count; only
selected sources perform analyzer work.

## Workflow

Run local CI (matches GitHub Actions):

    bash scripts/ci.sh

VST3 checks are opt-in:

    VST3_SDK_DIR=/Users/portalsurfer/lib/vst3sdk bash scripts/ci.sh --vst3

The GUI screenshot harness is opt-in:

    bash scripts/ci.sh --screenshots

To make a fresh host-installable VST3 artifact in this folder:

    DIST_DIR="$PWD/dist" \
      VST3_SDK_DIR=/Users/portalsurfer/lib/vst3sdk \
      bash ../scripts/build-vst3-release.sh Chromascope

The resulting bundle is dist/Chromascope-v0.1.0-macos.vst3.

This folder is an independently bootstrapped AudioDev repository. The local
`scripts/dist.sh` is intentionally VST3-only; production releases are built
and published by the checked-in GitHub Actions workflows:

- `.github/workflows/release-preflight.yml` builds an ad-hoc VST3 for review.
- `.github/workflows/release.yml` builds the signed/notarized VST3 and, when
  requested, publishes its manifest through PortalSurfer.
- `.github/workflows/nightly.yml` keeps the nightly release path available.

The staged bootstrap CLI also consumes `site/landing-page.json` to render the
PortalSurfer page and `site/product.json` to register the product. Plan mode is
the default for each stage; see `docs/RELEASE_CREDENTIALS.md` for the exact
configuration names without storing any secret values here.

## Docs

- Project notes: docs/PROJECT.md
- Development notes: docs/DEVELOPMENT.md
- Strict declarative checklist (meta-workspace): ../docs/STRICT-DECLARATIVE-CHECKLIST.md
