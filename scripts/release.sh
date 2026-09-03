#!/usr/bin/env bash
# AudioDev VST3-only release producer template; expanded by the bootstrap CLI.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
slug="chromascope"
endpoint="https://portalsurfer.org"
mode=""
channel="stable"
requested_version=""
requested_publication_version=""
build_id=""
released_at=""
source_ref=""
requested_source_sha=""
windows_release_dir=""
publisher_script=""
vst3_sdk_revision="58f8da7936800732561402d7936584ca4505de07"

usage() {
  cat <<'EOF'
Usage: scripts/release.sh (--package-only | --publish | --preflight) [options]

Options:
  --channel stable|rc|nightly  Release channel (default: stable)
  --version VERSION            Must match Cargo.toml
  --publication-version VERSION
                               Publication identity; core must match Cargo.toml
  --build-id ID                Immutable id (default: <slug>-v<publication-version>-<12-char HEAD>)
  --released-at ISO8601        Release timestamp (default: current UTC time)
  --endpoint URL               PortalSurfer origin (production is exact)
  --source-ref REF             Require a non-detached checkout of REF
  --source-sha SHA             Require this exact source SHA
  --windows-release-dir DIR    Validated Windows nightly artifact directory
  --publisher-script PATH      Pinned Node publisher script for --publish

Production stable/RC is macOS arm64, one signed/notarized VST3 ZIP, one fresh
screenshot, CHANGELOG.md, and one canonical manifest-v2 upload. Production
nightly additionally requires one explicitly unsigned Windows x86_64 VST3 ZIP
and emits one canonical manifest-v3 upload. Preflight produces an ad-hoc
unsigned/notarized schema-2 release for local inspection and never publishes.
Credentials are read only from the environment supplied by GitHub Actions.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --package-only|--publish|--preflight)
      [[ -z "${mode}" ]] || { echo "choose only one release mode" >&2; exit 2; }
      mode="${1#--}"
      shift
      ;;
    --channel) channel="${2:?missing channel}"; shift 2 ;;
    --version) requested_version="${2:?missing version}"; shift 2 ;;
    --publication-version) requested_publication_version="${2:?missing publication version}"; shift 2 ;;
    --build-id) build_id="${2:?missing build id}"; shift 2 ;;
    --released-at) released_at="${2:?missing released-at}"; shift 2 ;;
    --endpoint) endpoint="${2:?missing endpoint}"; shift 2 ;;
    --source-ref) source_ref="${2:?missing source ref}"; shift 2 ;;
    --source-sha) requested_source_sha="${2:?missing source sha}"; shift 2 ;;
    --windows-release-dir) windows_release_dir="${2:?missing Windows release directory}"; shift 2 ;;
    --publisher-script) publisher_script="${2:?missing publisher script}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

[[ -n "${mode}" ]] || { usage >&2; exit 2; }
[[ "${channel}" == stable || "${channel}" == rc || "${channel}" == nightly ]] || {
  echo "invalid channel: ${channel}" >&2
  exit 2
}
if [[ -n "${publisher_script}" && "${mode}" != publish ]]; then
  echo "--publisher-script requires --publish" >&2
  exit 2
fi
if [[ "${mode}" == preflight || "${channel}" != nightly ]]; then
  [[ -z "${windows_release_dir}" ]] || {
    echo "--windows-release-dir is only valid for a production nightly" >&2
    exit 2
  }
else
  [[ -n "${windows_release_dir}" ]] || {
    echo "production Chromascope nightly requires --windows-release-dir" >&2
    exit 1
  }
fi
[[ "$(uname -s)" == Darwin ]] || { echo "release packaging requires macOS" >&2; exit 1; }

if [[ -n "${source_ref}" ]]; then
  current_ref="$(git symbolic-ref --quiet --short HEAD || true)"
  [[ "${current_ref}" == "${source_ref}" ]] || {
    echo "requested source ${source_ref} does not match checkout ${current_ref:-detached}" >&2
    exit 1
  }
fi
[[ -z "$(git status --porcelain --untracked-files=all)" ]] || {
  echo "release source must be clean" >&2
  git status --short >&2
  exit 1
}

package_version="$(sed -n '/^\[package\]/,/^\[/ { s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p; }' Cargo.toml | head -n 1)"
[[ -n "${package_version}" ]] || { echo "Cargo.toml package version is missing" >&2; exit 1; }
if [[ -n "${requested_version}" && "${requested_version}" != "${package_version}" ]]; then
  echo "requested version ${requested_version} does not match Cargo.toml ${package_version}" >&2
  exit 1
fi
publication_version="${requested_publication_version:-${package_version}}"
PYTHONDONTWRITEBYTECODE=1 python3 - "${package_version}" "${publication_version}" "${channel}" <<'PY'
import pathlib
import sys
sys.path.insert(0, str(pathlib.Path("scripts").resolve()))
from release_helper import validate_publication_version
validate_publication_version(sys.argv[1], sys.argv[2], sys.argv[3])
PY

source_sha="$(git rev-parse HEAD)"
[[ "${source_sha}" =~ ^[0-9a-f]{40}$ ]] || { echo "could not resolve an exact source SHA" >&2; exit 1; }
if [[ -n "${requested_source_sha}" ]]; then
  [[ "${requested_source_sha}" =~ ^[0-9a-f]{40}$ && "${requested_source_sha}" == "${source_sha}" ]] || {
    echo "requested source SHA does not match checkout HEAD" >&2
    exit 1
  }
fi
if [[ "${mode}" != preflight ]]; then
  current_ref="$(git symbolic-ref --quiet --short HEAD || true)"
  [[ "${current_ref}" == main ]] || {
    echo "production release source must be a non-detached main checkout" >&2
    exit 1
  }
  git fetch origin main --quiet
  canonical_main="$(git rev-parse refs/remotes/origin/main 2>/dev/null || true)"
  [[ -n "${canonical_main}" && "${source_sha}" == "${canonical_main}" ]] || {
    echo "production release source must equal origin/main (${canonical_main:-unavailable})" >&2
    exit 1
  }
fi
build_id="${build_id:-${slug}-v${publication_version}-${source_sha:0:12}}"
[[ "${build_id}" =~ ^[a-z0-9][a-z0-9._-]{1,127}$ ]] || { echo "invalid build id" >&2; exit 2; }
if [[ "${mode}" != preflight && "${channel}" == nightly ]]; then
  expected_build_id="${slug}-v${publication_version}-${source_sha:0:12}"
  [[ "${build_id}" == "${expected_build_id}" ]] || {
    echo "production nightly build id must be ${expected_build_id}" >&2
    exit 1
  }
fi
released_at="${released_at:-$(date -u '+%Y-%m-%dT%H:%M:%SZ')}"
[[ -s CHANGELOG.md ]] || { echo "CHANGELOG.md must not be empty" >&2; exit 1; }

if [[ "${mode}" == publish ]]; then
  [[ -n "${PORTALSURFER_RELEASE_TOKEN:-}" ]] || {
    echo "--publish requires PORTALSURFER_RELEASE_TOKEN (environment only)" >&2
    exit 1
  }
  [[ "${endpoint}" == "https://portalsurfer.org" ]] || {
    echo "production publishing requires exact origin https://portalsurfer.org" >&2
    exit 1
  }
  if [[ -n "${publisher_script}" ]]; then
    [[ -f "${publisher_script}" && ! -L "${publisher_script}" ]] || {
      echo "--publisher-script must point to a regular file" >&2
      exit 1
    }
  elif [[ "${channel}" == nightly ]]; then
    echo "production nightly publishing requires the pinned Node publisher script" >&2
    exit 1
  fi
fi
: "${VST3_SDK_DIR:?VST3_SDK_DIR must point to a VST3 SDK checkout}"
[[ -d "${VST3_SDK_DIR}/pluginterfaces" ]] || {
  echo "VST3_SDK_DIR must contain pluginterfaces/" >&2
  exit 1
}

distribution="production"
if [[ "${mode}" == preflight ]]; then
  distribution="preflight"
else
  for required in \
    APPLE_DEVELOPER_ID_APPLICATION_CERT_BASE64 \
    APPLE_DEVELOPER_ID_APPLICATION_CERT_PASSWORD \
    APPLE_NOTARY_KEY_BASE64 APPLE_NOTARY_KEY_ID APPLE_NOTARY_ISSUER_ID; do
    [[ -n "${!required:-}" ]] || {
      echo "missing required Apple production credential: ${required}" >&2
      exit 1
    }
  done
fi

windows_release_root=""
windows_archive_name=""
if [[ "${distribution}" == production && "${channel}" == nightly ]]; then
  windows_release_root="$(cd "${windows_release_dir}" 2>/dev/null && pwd -P)" || {
    echo "Windows release directory is not available: ${windows_release_dir}" >&2
    exit 1
  }
  PYTHONDONTWRITEBYTECODE=1 python3 scripts/windows_release_helper.py validate \
    --root "${windows_release_root}" \
    --cargo-lock Cargo.lock \
    --vst3-sdk-revision "${vst3_sdk_revision}"
  PYTHONDONTWRITEBYTECODE=1 python3 - "${windows_release_root}" "${package_version}" "${publication_version}" "${build_id}" "${released_at}" "${source_sha}" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
package_version, publication_version, build_id, released_at, source_sha = sys.argv[2:]
manifest_path = root / "windows-artifact-manifest.json"
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
expected_archive = f"chromascope-v{publication_version}-windows-x86_64-unsigned.vst3.zip"
expected_build_id = f"chromascope-v{publication_version}-{source_sha[:12]}"
source = manifest.get("source")
if (
    manifest.get("schema_version") != 1
    or manifest.get("product") != "chromascope"
    or manifest.get("format") != "vst3"
    or manifest.get("platform") != "windows"
    or manifest.get("architecture") != "x86_64"
    or manifest.get("package_version") != package_version
    or manifest.get("publication_version") != publication_version
    or manifest.get("channel") != "nightly"
    or manifest.get("build_id") != expected_build_id
    or manifest.get("build_id") != build_id
    or manifest.get("released_at") != released_at
    or not isinstance(source, dict)
    or source.get("git_sha") != source_sha
    or source.get("repository") != "PORTALSURFER/chromascope"
    or source.get("dirty") is not False
    or manifest.get("signing_status") != "unsigned"
    or manifest.get("signing_certificate") is not None
    or not isinstance(manifest.get("archive"), dict)
    or manifest["archive"].get("name") != expected_archive
):
    raise SystemExit("Windows artifact manifest does not match the shared nightly identity")
entries = list(root.iterdir())
expected_entries = {"windows-artifact-manifest.json", expected_archive}
if {entry.name for entry in entries} != expected_entries or any(entry.is_symlink() or not entry.is_file() for entry in entries):
    raise SystemExit("Windows release directory must contain only the archive and its sidecar")
PY
  windows_archive_name="chromascope-v${publication_version}-windows-x86_64-unsigned.vst3.zip"
fi

release_dir="${repo_root}/dist/releases/${build_id}"
rm -rf -- "${release_dir}"
mkdir -p "${release_dir}" "${repo_root}/target"
tmp_root="$(mktemp -d "${repo_root}/target/release-build.XXXXXX")"
original_keychains=()
release_keychain=""
original_keychains_file=""
cleanup() {
  if [[ -f "${original_keychains_file}" && "${#original_keychains[@]}" -gt 0 ]]; then
    security list-keychains -d user -s "${original_keychains[@]}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${release_keychain}" ]]; then
    security delete-keychain "${release_keychain}" >/dev/null 2>&1 || true
  fi
  rm -rf -- "${tmp_root}"
}
trap cleanup EXIT

if [[ -n "${windows_archive_name}" ]]; then
  cp "${windows_release_root}/${windows_archive_name}" "${release_dir}/${windows_archive_name}"
fi

signing_team_id=""
vst3_notary_id=""
codesign_identity="-"
if [[ "${distribution}" == production ]]; then
  decode_base64() {
    if printf '%s' "$1" | base64 --decode > "$2" 2>/dev/null; then return 0; fi
    printf '%s' "$1" | base64 -D > "$2"
  }
  cert_path="${tmp_root}/developer-id-application.p12"
  notary_key_path="${tmp_root}/AuthKey_${APPLE_NOTARY_KEY_ID}.p8"
  decode_base64 "${APPLE_DEVELOPER_ID_APPLICATION_CERT_BASE64}" "${cert_path}"
  decode_base64 "${APPLE_NOTARY_KEY_BASE64}" "${notary_key_path}"
  chmod 600 "${cert_path}" "${notary_key_path}"
  release_keychain="${tmp_root}/release.keychain-db"
  release_keychain_password="$(uuidgen | tr -d '-')"
  original_keychains_file="${tmp_root}/original-keychains.txt"
  security list-keychains -d user | sed 's/[[:space:]]*"//g; s/"$//' > "${original_keychains_file}"
  while IFS= read -r item; do [[ -n "${item}" ]] && original_keychains+=("${item}"); done < "${original_keychains_file}"
  security create-keychain -p "${release_keychain_password}" "${release_keychain}" >/dev/null
  security set-keychain-settings -lut 21600 "${release_keychain}"
  security unlock-keychain -p "${release_keychain_password}" "${release_keychain}"
  security list-keychains -d user -s "${release_keychain}" "${original_keychains[@]}" >/dev/null
  security import "${cert_path}" -P "${APPLE_DEVELOPER_ID_APPLICATION_CERT_PASSWORD}" -A -t cert -f pkcs12 -k "${release_keychain}" >/dev/null
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "${release_keychain_password}" "${release_keychain}" >/dev/null
  codesign_identity="${APPLE_CODESIGN_IDENTITY:-}"
  if [[ -z "${codesign_identity}" ]]; then
    codesign_identity="$(security find-identity -v -p codesigning "${release_keychain}" | sed -n 's/.*"\(Developer ID Application:.*\)".*/\1/p' | head -n 1)"
  fi
  [[ "${codesign_identity}" == Developer\ ID\ Application:* ]] || {
    echo "no Developer ID Application identity found" >&2
    exit 1
  }
fi

rm -rf -- target/ui-screenshots
bash scripts/ci.sh --screenshots
screenshot_source="$(find target/ui-screenshots -type f -name 'initial-ui-*.png' -print | sort | head -n 1)"
[[ -n "${screenshot_source}" && -f "${screenshot_source}" ]] || {
  echo "release requires one screenshot named *-default-WIDTHxHEIGHT.png" >&2
  exit 1
}
screenshot_dimensions="$(python3 - "${screenshot_source}" <<'PY'
import struct
import sys
data = open(sys.argv[1], "rb").read()
if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
    raise SystemExit("screenshot is not a PNG with an IHDR")
width, height = struct.unpack(">II", data[16:24])
print(f"{width}x{height}")
PY
)"
screenshot_name="${slug}-default-${screenshot_dimensions}.png"
cp "${screenshot_source}" "${release_dir}/${screenshot_name}"

package_name="$(sed -n '/^\[package\]/,/^\[/ { s/^name[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p; }' Cargo.toml | head -n 1)"
lib_name="${package_name//-/_}"
vst3_target="${tmp_root}/vst3-target"
VST3_SDK_DIR="${VST3_SDK_DIR}" CARGO_TARGET_DIR="${vst3_target}" \
  cargo rustc --release --features vst3 --lib -- -C link-arg=-Wl,-bundle
vst3_binary="${vst3_target}/release/lib${lib_name}.dylib"
[[ -f "${vst3_binary}" ]] || { echo "missing VST3 build output: ${vst3_binary}" >&2; exit 1; }
/usr/bin/file "${vst3_binary}" | grep -q arm64 || {
  echo "VST3 release binary must contain arm64" >&2
  exit 1
}
for symbol in _GetPluginFactory _bundleEntry _bundleExit; do
  /usr/bin/nm -gU "${vst3_binary}" | grep -q "${symbol}" || {
    echo "VST3 entrypoint ${symbol} missing from binary" >&2
    exit 1
  }
done

bundle="${tmp_root}/${slug}.vst3"
mkdir -p "${bundle}/Contents/MacOS"
cp "${vst3_binary}" "${bundle}/Contents/MacOS/${slug}"
chmod 755 "${bundle}/Contents/MacOS/${slug}"
cat > "${bundle}/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleDisplayName</key><string>Chromascope</string>
<key>CFBundleExecutable</key><string>${slug}</string>
<key>CFBundleIdentifier</key><string>com.portalsurfer.${slug}.vst3</string>
<key>CFBundleName</key><string>Chromascope</string>
<key>CFBundlePackageType</key><string>BNDL</string>
<key>CFBundleShortVersionString</key><string>${package_version}</string>
<key>CFBundleVersion</key><string>${package_version}</string>
<key>CFBundleSupportedPlatforms</key><array><string>MacOSX</string></array>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
printf 'BNDL????' > "${bundle}/Contents/PkgInfo"
/usr/bin/plutil -lint "${bundle}/Contents/Info.plist" >/dev/null
if [[ "${distribution}" == production ]]; then
  codesign --force --deep --timestamp --options runtime --keychain "${release_keychain}" --sign "${codesign_identity}" "${bundle}" >/dev/null
  codesign --verify --deep --strict "${bundle}"
  /usr/bin/ditto -c -k --sequesterRsrc --keepParent "${bundle}" "${tmp_root}/notary.vst3.zip"
  notary_json="${tmp_root}/notary.json"
  xcrun notarytool submit "${tmp_root}/notary.vst3.zip" --key "${notary_key_path}" --key-id "${APPLE_NOTARY_KEY_ID}" --issuer "${APPLE_NOTARY_ISSUER_ID}" --wait --output-format json > "${notary_json}"
  notary_status="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["status"])' "${notary_json}")"
  vst3_notary_id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "${notary_json}")"
  [[ "${notary_status}" == Accepted ]] || { echo "VST3 notarization was not accepted" >&2; exit 1; }
  xcrun stapler staple "${bundle}" >/dev/null
  xcrun stapler validate "${bundle}" >/dev/null
  codesign -vvvv -R=notarized --check-notarization "${bundle}" >/dev/null
  signing_team_id="$(codesign -dv --verbose=4 "${bundle}" 2>&1 | sed -n 's/^TeamIdentifier=//p' | head -n 1)"
  [[ "${signing_team_id}" =~ ^[A-Z0-9]{10}$ ]] || { echo "could not capture Developer ID team identifier" >&2; exit 1; }
else
  codesign --force --deep --sign - "${bundle}" >/dev/null
  codesign --verify --deep --strict "${bundle}"
fi

archive="${release_dir}/${slug}-v${publication_version}-macos.vst3.zip"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "${bundle}" "${archive}"
/usr/bin/ditto -x -k "${archive}" "${tmp_root}/audit"
audit_bundle="${tmp_root}/audit/${slug}.vst3"
test -x "${audit_bundle}/Contents/MacOS/${slug}"
/usr/bin/plutil -lint "${audit_bundle}/Contents/Info.plist" >/dev/null
[[ "$(/usr/bin/plutil -extract CFBundleIdentifier raw -o - "${audit_bundle}/Contents/Info.plist")" == "com.portalsurfer.${slug}.vst3" ]] || {
  echo "VST3 bundle identifier is invalid" >&2
  exit 1
}
codesign --verify --deep --strict "${audit_bundle}"
if [[ "${distribution}" == production ]]; then
  xcrun stapler validate "${audit_bundle}" >/dev/null
  codesign -vvvv -R=notarized --check-notarization "${audit_bundle}" >/dev/null
fi

cp CHANGELOG.md "${release_dir}/CHANGELOG.md"
PYTHONDONTWRITEBYTECODE=1 python3 - "${release_dir}" "${publication_version}" "${build_id}" "${channel}" "${released_at}" "${source_sha}" "${distribution}" "${signing_team_id}" "${vst3_notary_id}" "${windows_archive_name}" <<'PY'
import pathlib
import sys

folder = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(folder.parents[1].parent / "scripts"))
from release_helper import build_manifest, canonical_json, validate_manifest

out, publication_version, build_id, channel, released_at, source_sha, distribution, team_id, notary_id, windows_archive_name = sys.argv[1:]
vst3 = folder / f"chromascope-v{publication_version}-macos.vst3.zip"
screenshot = next(folder.glob("chromascope-default-*.png"))
windows_vst3 = folder / windows_archive_name if windows_archive_name else None
manifest = build_manifest(
    product="chromascope",
    repository="PORTALSURFER/chromascope",
    version=publication_version,
    build_id=build_id,
    channel=channel,
    released_at=released_at,
    git_sha=source_sha,
    vst3=vst3,
    screenshot=screenshot,
    changelog=folder / "CHANGELOG.md",
    distribution=distribution,
    signing_team_id=team_id,
    vst3_notary_id=notary_id,
    windows_vst3=windows_vst3,
)
(folder / "release-manifest.json").write_bytes(canonical_json(manifest))
validate_manifest(manifest, folder)
PY

if [[ "${mode}" == publish ]]; then
  if [[ -n "${publisher_script}" ]]; then
    node "${publisher_script}" \
      --manifest "${release_dir}/release-manifest.json" \
      --root "${release_dir}" \
      --endpoint "${endpoint}"
  else
    PYTHONDONTWRITEBYTECODE=1 python3 - "${release_dir}" "${endpoint}" <<'PY'
import json
import os
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(root.parents[1].parent / "scripts"))
from release_helper import publish_release

publish_release(
    endpoint=sys.argv[2],
    token=os.environ.get("PORTALSURFER_RELEASE_TOKEN", ""),
    manifest=json.loads((root / "release-manifest.json").read_text(encoding="utf-8")),
    root=root,
    repo_root=root.parents[2],
)
PY
  fi
fi
echo "[release] ${mode} VST3 bundle ready: ${release_dir}"
