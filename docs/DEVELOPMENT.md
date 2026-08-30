# Development

This folder is a self-contained Toybox plugin project inside the AudioDev
meta-workspace. The Toybox dependency is pinned to a full git revision in
Cargo.toml; do not replace it with a path dependency or commit a Cargo
[patch] override.

## Local checks

    bash scripts/ci.sh

The focused commands used for the prototype are:

    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --features vst3

The screenshot lane needs a compatible headless graphics runtime:

    bash scripts/ci.sh --screenshots

Build a fresh macOS VST3 bundle into this plugin's own dist directory:

    DIST_DIR="$PWD/dist" \
      VST3_SDK_DIR=/Users/portalsurfer/lib/vst3sdk \
      bash ../scripts/build-vst3-release.sh Chromascope

The build script validates the Mach-O bundle exports, writes the VST3
Info.plist, and ad-hoc signs the result. Manual DAW acceptance remains a
host-side test; do not use the build scripts to operate an active DAW session.

Companion scaling is intentionally bounded at 128 process-local registrations.
The UI keeps metadata for every registered source but the native editor paints
only the visible scrolled rows. A companion's 8192-sample analyzer and mailbox
publication become active only when an open viewer selects that source; the
interest count is atomic in the audio path and its registry bookkeeping is
control/UI-only.

The analyzer's raw display floor is -100 dBFS. Values at or below that floor
stay flat and bypass the +4.5 dB/oct presentation tilt, which keeps silence
visibly silent while preserving above-floor low-level signals. Timed frames use
the VST3 process context's continuous sample position, with project sample time
or an explicit untimed latest-frame fallback when needed. The fixed eight-frame
mailbox history supports common-time selection without locks or audio-thread
allocation.

The native Radiant editor keeps a separate UI presentation state for the main
trace and each selected companion. It targets a 60 Hz repaint interval when
the host/editor provides one, uses 20 ms attack and 100 ms release ballistics,
and caps a delayed repaint step at 50 ms. This is presentation-only: the DSP
FFT remains 8192 samples with a 2048-sample hop, and no additional audio
latency is introduced. The pinned Toybox macOS bridge currently supplies a
generic approximately 30 Hz fallback timer, so the artifact is 60 Hz-ready but
cannot force a host/framework timer to run faster.

The visual contract is centralized in `src/visual_system.rs` and mirrors Pump's
dark-coral Radiant tokens: charcoal `#1B1E1E` surfaces, `#3A3D3D` borders,
`#D8D7D3`/`#999B9A` text, `#363939`/`#282B2B` grid, and `#E95843`/`#F16C56`
coral accents. The viewer's fixed main trace uses the primary coral; companion
colors remain stable per-source accents. Native spacing, rounded surfaces, and
typography use Pump's compact 3.4/6.8/10.2/13.6 roles. The declarative
fallback receives the same semantic colors through its root `ThemeTokens`.

Source activation is independent from visual highlighting. Normal source-row
clicks control analyzer interest, while Command-click toggles one persistent
highlight and activates the source when the highlight is added. Removing a
highlight does not deactivate its source. Eight active sources can be highlighted
simultaneously; every entry keeps a stable, visibly tinted light color shared by
its source row and scope trace. The UI reports a full set without replacing an
existing entry, removes highlights for sources that become inactive or leave the
registry, and keeps all highlight state off the audio callback.

## VST3 editor paths

On macOS, the VST3 viewer uses Toybox's `radiant-vst3` AppKit bridge. Ableton
and other macOS hosts provide an AppKit parent view, which the bridge wraps in
the Toybox Radiant editor and repaints from the shared registry/mailboxes. The
legacy Patchbay editor remains the portable declarative preview path for the
headless screenshot lane and non-macOS builds; both paths expose the same
viewer behavior, source selection, and colors.

## Working against local Toybox (local-only)

See the meta-root DEVELOPMENT.md for the approved local-only patch workflow.
