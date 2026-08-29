#!/usr/bin/env bash
# Local, unsigned AudioDev VST3 distribution/test producer.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
slug="chromascope"
display_name="Chromascope"
output_dir="${AUDIODEV_DIST_DIR:-${repo_root}/dist}"
run_checks=true

usage() {
  cat <<EOF
Usage: scripts/dist.sh [--output-dir PATH] [--skip-checks]

Builds the unsigned local macOS VST3 bundle for host testing under dist/ (or
the explicit output directory). This command never contacts GitHub, Apple, or
PortalSurfer and never consumes production signing credentials.

This product is VST3-only; CLAP is intentionally not a supported format.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-dir) output_dir="${2:?missing output directory}"; shift 2 ;;
    --skip-checks) run_checks=false; shift ;;
    --format)
      format="${2:?missing format}"
      [[ "${format}" == vst3 ]] || {
        echo "this product supports only --format vst3" >&2
        exit 2
      }
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

[[ "$(uname -s)" == Darwin ]] || {
  echo "local VST3 bundle packaging requires macOS" >&2
  exit 1
}
: "${VST3_SDK_DIR:?VST3_SDK_DIR must point to a VST3 SDK checkout}"
[[ -d "${VST3_SDK_DIR}/pluginterfaces" ]] || {
  echo "VST3_SDK_DIR must contain pluginterfaces/" >&2
  exit 1
}

if [[ "${run_checks}" == true ]]; then
  bash scripts/ci.sh
  VST3_SDK_DIR="${VST3_SDK_DIR}" bash scripts/ci.sh --vst3
fi

package_name="$(sed -n '/^\[package\]/,/^\[/ { s/^name[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p; }' Cargo.toml | head -n 1)"
version="$(sed -n '/^\[package\]/,/^\[/ { s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p; }' Cargo.toml | head -n 1)"
[[ -n "${package_name}" && -n "${version}" ]] || {
  echo "Cargo.toml package name/version is missing" >&2
  exit 1
}
lib_name="${package_name//-/_}"
mkdir -p "${output_dir}"

VST3_SDK_DIR="${VST3_SDK_DIR}" \
  cargo rustc --release --features vst3 --lib -- -C link-arg=-Wl,-bundle
vst3_binary="target/release/lib${lib_name}.dylib"
[[ -f "${vst3_binary}" ]] || {
  echo "missing VST3 build output: ${vst3_binary}" >&2
  exit 1
}
/usr/bin/file "${vst3_binary}" | grep -q arm64 || {
  echo "VST3 local build must contain arm64" >&2
  exit 1
}
for symbol in _GetPluginFactory _bundleEntry _bundleExit; do
  /usr/bin/nm -gU "${vst3_binary}" | grep -q "${symbol}" || {
    echo "VST3 symbol ${symbol} missing from ${vst3_binary}" >&2
    exit 1
  }
done

bundle="${output_dir}/${slug}-v${version}-macos.vst3"
[[ ! -e "${bundle}" ]] || {
  echo "refusing to overwrite existing local bundle: ${bundle}" >&2
  exit 1
}
mkdir -p "${bundle}/Contents/MacOS"
cp "${vst3_binary}" "${bundle}/Contents/MacOS/${slug}"
chmod 755 "${bundle}/Contents/MacOS/${slug}"
cat > "${bundle}/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleDisplayName</key><string>${display_name}</string>
<key>CFBundleExecutable</key><string>${slug}</string>
<key>CFBundleIdentifier</key><string>com.portalsurfer.${slug}.vst3</string>
<key>CFBundleName</key><string>${display_name}</string>
<key>CFBundlePackageType</key><string>BNDL</string>
<key>CFBundleShortVersionString</key><string>${version}</string>
<key>CFBundleVersion</key><string>${version}</string>
<key>CFBundleSupportedPlatforms</key><array><string>MacOSX</string></array>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
printf 'BNDL????' > "${bundle}/Contents/PkgInfo"
/usr/bin/plutil -lint "${bundle}/Contents/Info.plist" >/dev/null
codesign --force --deep --sign - "${bundle}" >/dev/null
codesign --verify --deep --strict "${bundle}"
echo "wrote ${bundle}"
/usr/bin/shasum -a 256 "${bundle}/Contents/MacOS/${slug}"
