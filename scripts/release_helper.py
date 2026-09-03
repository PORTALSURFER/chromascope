#!/usr/bin/env python3
"""Manifest-v2/v3 and PortalSurfer transport helpers for a VST3-only plug-in."""

from __future__ import annotations

import datetime as dt
import hashlib
import json
import re
import struct
import subprocess
import zlib
from pathlib import Path
from typing import Any, Callable, Optional
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

MANIFEST_SCHEMA = 2
MANIFEST_SCHEMA_V2 = 2
MANIFEST_SCHEMA_V3 = 3
MANIFEST_CONTENT_TYPES = {
    MANIFEST_SCHEMA_V2: "application/vnd.portalsurfer.release-manifest+json;version=2",
    MANIFEST_SCHEMA_V3: "application/vnd.portalsurfer.release-manifest+json;version=3",
}
MANIFEST_CONTENT_TYPE = MANIFEST_CONTENT_TYPES[MANIFEST_SCHEMA]
PRODUCTION_ORIGIN = "https://portalsurfer.org"
CHROMASCOPE_TEAM_ID = "DKTKQ8U5T8"
SAFE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
SAFE_BUILD_ID = re.compile(r"^[a-z0-9][a-z0-9._-]{1,127}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
TEAM_ID = re.compile(r"^[A-Z0-9]{10}$")
NOTARY_ID = re.compile(
    r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-"
    r"[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$"
)
BASE_VERSION = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
SEMVER = re.compile(rf"{BASE_VERSION}(?:-[0-9A-Za-z.-]+)?\Z")
CORE_SEMVER = re.compile(rf"{BASE_VERSION}\Z")
RELEASE_CHANNELS = frozenset(("stable", "rc", "nightly"))
CHANNEL_VERSION_PATTERNS = {
    "stable": re.compile(rf"{BASE_VERSION}\Z"),
    "rc": re.compile(rf"{BASE_VERSION}-rc\.[1-9][0-9]*\Z"),
    "nightly": re.compile(rf"{BASE_VERSION}-nightly\.[1-9][0-9]*\Z"),
}
WORKFLOW_SEQUENCE = re.compile(r"[1-9][0-9]*\Z")
SCREENSHOT_NAME = re.compile(r"^[a-z0-9][a-z0-9-]{0,62}-default-[0-9]+x[0-9]+\.png$")


def _parse_core_version(version: Any, *, field: str) -> tuple[int, int, int]:
    if not isinstance(version, str) or CORE_SEMVER.fullmatch(version) is None:
        raise ValueError(f"{field} must be a numeric semver")
    return tuple(int(part) for part in version.split("."))


def _parse_channel_version(version: Any, channel: Any, *, field: str) -> tuple[int, int, int]:
    if not isinstance(channel, str) or channel not in CHANNEL_VERSION_PATTERNS:
        raise ValueError(f"{field} has an invalid release channel")
    if not isinstance(version, str) or CHANNEL_VERSION_PATTERNS[channel].fullmatch(version) is None:
        raise ValueError(f"{field} does not match the {channel} release version syntax")
    return _parse_core_version(version.split("-", 1)[0], field=field)


def validate_channel_version(version: str, channel: str) -> None:
    _parse_channel_version(version, channel, field="publication version")


def validate_publication_version(package_version: str, publication_version: str, channel: str) -> None:
    package_core = _parse_core_version(package_version, field="package version")
    publication_core = _parse_channel_version(publication_version, channel, field="publication version")
    if publication_core != package_core:
        raise ValueError(f"publication version {publication_version} does not match package version {package_version}")


def derive_publication_version(package_version: str, channel: str, sequence: Any) -> str:
    if not isinstance(channel, str) or channel not in RELEASE_CHANNELS:
        raise ValueError(f"invalid release channel: {channel}")
    package_text = ".".join(str(part) for part in _parse_core_version(package_version, field="package version"))
    if channel == "stable":
        publication_version = package_text
    else:
        if not isinstance(sequence, (str, int)) or isinstance(sequence, bool) or WORKFLOW_SEQUENCE.fullmatch(str(sequence)) is None:
            raise ValueError("non-stable publication versions require a positive workflow sequence")
        publication_version = f"{package_text}-{channel}.{sequence}"
    validate_publication_version(package_text, publication_version, channel)
    return publication_version


def canonical_json(value: Any) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def file_digest(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
    return digest.hexdigest(), size


def validate_png(path: Path) -> tuple[int, int, str, int]:
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("screenshot is not a PNG")
    offset = 8
    dimensions: tuple[int, int] | None = None
    seen_ihdr = seen_idat = seen_iend = False
    while offset + 12 <= len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        kind = data[offset + 4 : offset + 8]
        end = offset + length + 12
        if end > len(data):
            raise ValueError("screenshot has a truncated PNG chunk")
        payload = data[offset + 8 : offset + 8 + length]
        actual_crc = struct.unpack(">I", data[offset + 8 + length : end])[0]
        if actual_crc != (zlib.crc32(kind + payload) & 0xFFFFFFFF):
            raise ValueError("screenshot has an invalid PNG chunk CRC")
        if kind == b"IHDR":
            if offset != 8 or seen_ihdr or length != 13:
                raise ValueError("screenshot has an invalid IHDR")
            width, height = struct.unpack(">II", payload[:8])
            if not width or not height or payload[8] != 8 or payload[9] not in (2, 6) or payload[10:] != b"\x00\x00\x00":
                raise ValueError("screenshot must be an 8-bit RGB/RGBA PNG")
            dimensions = (width, height)
            seen_ihdr = True
        elif kind == b"IDAT":
            if not seen_ihdr:
                raise ValueError("screenshot IDAT precedes IHDR")
            seen_idat = True
        elif kind == b"IEND":
            if length != 0 or end != len(data):
                raise ValueError("screenshot has an invalid IEND")
            seen_iend = True
            break
        offset = end
    if dimensions is None or not seen_idat or not seen_iend:
        raise ValueError("screenshot is missing IHDR, IDAT, or IEND")
    digest, size = file_digest(path)
    return dimensions[0], dimensions[1], digest, size


def _validate_common(product: str, repository: str, version: str, build_id: str, channel: str, released_at: str, git_sha: str) -> None:
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,62}", product):
        raise ValueError("invalid product slug")
    if repository != f"PORTALSURFER/{product}":
        raise ValueError("unknown or mismatched Portal product")
    if not isinstance(version, str) or SEMVER.fullmatch(version) is None or "+" in version:
        raise ValueError("version must be SemVer without build metadata")
    if channel not in RELEASE_CHANNELS:
        raise ValueError("invalid release channel")
    _parse_channel_version(version, channel, field="version")
    if not re.fullmatch(r"[0-9a-f]{40}", git_sha) or not SAFE_BUILD_ID.fullmatch(build_id):
        raise ValueError("source SHA or build id is invalid")
    try:
        parsed = dt.datetime.fromisoformat(released_at.replace("Z", "+00:00"))
    except ValueError as error:
        raise ValueError("released_at must be RFC3339") from error
    if parsed.tzinfo is None:
        raise ValueError("released_at must include a timezone")


def _build_schema2_manifest(
    *,
    product: str,
    repository: str,
    version: str,
    build_id: str,
    channel: str,
    released_at: str,
    git_sha: str,
    vst3: Path,
    screenshot: Path,
    changelog: Path,
    distribution: str = "production",
    signing_team_id: str = "",
    vst3_notary_id: str = "",
) -> dict[str, Any]:
    _validate_common(product, repository, version, build_id, channel, released_at, git_sha)
    if distribution not in {"production", "preflight"}:
        raise ValueError("invalid distribution")
    if distribution == "production":
        if not TEAM_ID.fullmatch(signing_team_id) or not NOTARY_ID.fullmatch(vst3_notary_id):
            raise ValueError("production signing/notarization evidence is incomplete")
        signing = {
            "identity_class": "Developer ID Application",
            "notarized": True,
            "stapled": True,
            "team_id": signing_team_id,
            "notary_submissions": {"vst3": vst3_notary_id},
        }
    else:
        if signing_team_id or vst3_notary_id:
            raise ValueError("preflight manifests must not contain production evidence")
        signing = {
            "identity_class": "ad hoc",
            "notarized": False,
            "stapled": False,
            "team_id": "",
            "notary_submissions": {},
        }
    expected_artifact = f"{product}-v{version}-macos.vst3.zip"
    if vst3.name != expected_artifact or not SAFE_NAME.fullmatch(vst3.name):
        raise ValueError(f"VST3 artifact must be named {expected_artifact}")
    artifact_hash, artifact_size = file_digest(vst3)
    if artifact_size <= 0 or not SHA256.fullmatch(artifact_hash):
        raise ValueError("VST3 artifact is empty or has an invalid hash")
    if screenshot.name != f"{product}-default-{screenshot.name.rsplit('-', 1)[-1]}":
        raise ValueError("screenshot name is not bound to the product")
    if not SCREENSHOT_NAME.fullmatch(screenshot.name):
        raise ValueError("screenshot name is invalid")
    width, height, screenshot_hash, screenshot_size = validate_png(screenshot)
    expected_dimensions = f"{width}x{height}.png"
    if not screenshot.name.endswith(expected_dimensions):
        raise ValueError("screenshot name dimensions do not match PNG dimensions")
    changelog_hash, changelog_size = file_digest(changelog)
    if changelog.name != "CHANGELOG.md" or changelog_size <= 0 or not SHA256.fullmatch(changelog_hash):
        raise ValueError("CHANGELOG.md is missing or invalid")
    names = [vst3.name, screenshot.name, changelog.name]
    if len(names) != len(set(names)):
        raise ValueError("release file names must be unique")
    return {
        "schema_version": MANIFEST_SCHEMA,
        "product": product,
        "build_id": build_id,
        "version": version,
        "channel": channel,
        "released_at": released_at,
        "source": {"repository": repository, "git_sha": git_sha, "dirty": False},
        "distribution": distribution,
        "signing": signing,
        "artifacts": [{
            "format": "vst3",
            "platform": "macos",
            "architectures": ["arm64"],
            "name": vst3.name,
            "media_type": "application/zip",
            "sha256": artifact_hash,
            "size_bytes": artifact_size,
        }],
        "screenshot": {
            "role": "default-ui",
            "name": screenshot.name,
            "media_type": "image/png",
            "width": width,
            "height": height,
            "logical_width": width,
            "logical_height": height,
            "dpi_scale": 1.0,
            "source_git_sha": git_sha,
            "sha256": screenshot_hash,
            "size_bytes": screenshot_size,
        },
        "changelog": {
            "name": "CHANGELOG.md",
            "format": "markdown",
            "media_type": "text/markdown; charset=utf-8",
            "sha256": changelog_hash,
            "size_bytes": changelog_size,
        },
    }


def build_manifest(
    *,
    product: str,
    repository: str,
    version: str,
    build_id: str,
    channel: str,
    released_at: str,
    git_sha: str,
    vst3: Path,
    screenshot: Path,
    changelog: Path,
    distribution: str = "production",
    signing_team_id: str = "",
    vst3_notary_id: str = "",
    windows_vst3: Optional[Path] = None,
) -> dict[str, Any]:
    _validate_common(product, repository, version, build_id, channel, released_at, git_sha)
    if distribution not in {"production", "preflight"}:
        raise ValueError("invalid distribution")
    if windows_vst3 is None:
        if product == "chromascope" and channel == "nightly" and distribution == "production":
            raise ValueError("production Chromascope nightly requires the Windows artifact")
        return _build_schema2_manifest(
            product=product,
            repository=repository,
            version=version,
            build_id=build_id,
            channel=channel,
            released_at=released_at,
            git_sha=git_sha,
            vst3=vst3,
            screenshot=screenshot,
            changelog=changelog,
            distribution=distribution,
            signing_team_id=signing_team_id,
            vst3_notary_id=vst3_notary_id,
        )
    if product != "chromascope" or channel != "nightly" or distribution != "production":
        raise ValueError("schema 3 is only available for production Chromascope nightlies")
    return _build_schema3_manifest(
        product=product,
        repository=repository,
        version=version,
        build_id=build_id,
        channel=channel,
        released_at=released_at,
        git_sha=git_sha,
        vst3=vst3,
        screenshot=screenshot,
        changelog=changelog,
        signing_team_id=signing_team_id,
        vst3_notary_id=vst3_notary_id,
        windows_vst3=windows_vst3,
    )


def _validate_regular_file(path: Path, label: str) -> None:
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"{label} must be a regular file: {path.name}")


def _validated_file_digest(path: Path, label: str) -> tuple[str, int]:
    _validate_regular_file(path, label)
    digest, size = file_digest(path)
    if size <= 0 or not SHA256.fullmatch(digest):
        raise ValueError(f"{label} is empty or has an invalid hash")
    return digest, size


def _schema3_asset_metadata(
    *, product: str, git_sha: str, screenshot: Path, changelog: Path
) -> tuple[dict[str, Any], dict[str, Any]]:
    if screenshot.name != f"{product}-default-{screenshot.name.rsplit('-', 1)[-1]}":
        raise ValueError("screenshot name is not bound to the product")
    if not SCREENSHOT_NAME.fullmatch(screenshot.name):
        raise ValueError("screenshot name is invalid")
    _validate_regular_file(screenshot, "screenshot")
    width, height, screenshot_hash, screenshot_size = validate_png(screenshot)
    if not screenshot.name.endswith(f"{width}x{height}.png"):
        raise ValueError("screenshot name dimensions do not match PNG dimensions")
    if changelog.name != "CHANGELOG.md":
        raise ValueError("CHANGELOG.md is missing or invalid")
    changelog_hash, changelog_size = _validated_file_digest(changelog, "CHANGELOG.md")
    return (
        {
            "role": "default-ui",
            "name": screenshot.name,
            "media_type": "image/png",
            "width": width,
            "height": height,
            "logical_width": width,
            "logical_height": height,
            "dpi_scale": 1.0,
            "source_git_sha": git_sha,
            "sha256": screenshot_hash,
            "size_bytes": screenshot_size,
        },
        {
            "name": "CHANGELOG.md",
            "format": "markdown",
            "media_type": "text/markdown; charset=utf-8",
            "sha256": changelog_hash,
            "size_bytes": changelog_size,
        },
    )


def _build_schema3_manifest(
    *,
    product: str,
    repository: str,
    version: str,
    build_id: str,
    channel: str,
    released_at: str,
    git_sha: str,
    vst3: Path,
    screenshot: Path,
    changelog: Path,
    signing_team_id: str,
    vst3_notary_id: str,
    windows_vst3: Path,
) -> dict[str, Any]:
    expected_build_id = f"chromascope-v{version}-{git_sha[:12]}"
    if build_id != expected_build_id:
        raise ValueError(f"schema 3 build id must be {expected_build_id}")
    if signing_team_id != CHROMASCOPE_TEAM_ID:
        raise ValueError(f"schema 3 requires Apple team ID {CHROMASCOPE_TEAM_ID}")
    if not NOTARY_ID.fullmatch(vst3_notary_id):
        raise ValueError("production signing/notarization evidence is incomplete")

    expected_macos_name = f"{product}-v{version}-macos.vst3.zip"
    if vst3.name != expected_macos_name or not SAFE_NAME.fullmatch(vst3.name):
        raise ValueError(f"VST3 artifact must be named {expected_macos_name}")
    macos_hash, macos_size = _validated_file_digest(vst3, "VST3 artifact")

    expected_windows_name = f"{product}-v{version}-windows-x86_64-unsigned.vst3.zip"
    if windows_vst3.name != expected_windows_name or not SAFE_NAME.fullmatch(windows_vst3.name):
        raise ValueError(f"Windows VST3 artifact must be named {expected_windows_name}")
    windows_hash, windows_size = _validated_file_digest(windows_vst3, "Windows VST3 artifact")

    screenshot_metadata, changelog_metadata = _schema3_asset_metadata(
        product=product,
        git_sha=git_sha,
        screenshot=screenshot,
        changelog=changelog,
    )
    artifacts = [
        {
            "format": "vst3",
            "platform": "macos",
            "architectures": ["arm64"],
            "name": vst3.name,
            "media_type": "application/zip",
            "sha256": macos_hash,
            "size_bytes": macos_size,
            "security": {
                "status": "signed",
                "certificate": "Developer ID Application",
                "team_id": CHROMASCOPE_TEAM_ID,
                "notarized": True,
                "stapled": True,
                "notary_submission": vst3_notary_id,
            },
        },
        {
            "format": "vst3",
            "platform": "windows",
            "architectures": ["x86_64"],
            "name": windows_vst3.name,
            "media_type": "application/zip",
            "sha256": windows_hash,
            "size_bytes": windows_size,
            "security": {"status": "unsigned", "certificate": None},
        },
    ]
    names = [artifact["name"] for artifact in artifacts] + [screenshot.name, changelog.name]
    if len(names) != len(set(names)):
        raise ValueError("release file names must be unique")
    return {
        "schema_version": MANIFEST_SCHEMA_V3,
        "product": product,
        "build_id": build_id,
        "version": version,
        "channel": channel,
        "released_at": released_at,
        "source": {"repository": repository, "git_sha": git_sha, "dirty": False},
        "distribution": "production",
        "artifacts": artifacts,
        "screenshot": screenshot_metadata,
        "changelog": changelog_metadata,
    }


def _validate_schema2_manifest(manifest: dict[str, Any], root: Path) -> None:
    required = {"schema_version", "product", "build_id", "version", "channel", "released_at", "source", "distribution", "signing", "artifacts", "screenshot", "changelog"}
    if set(manifest) != required or manifest.get("schema_version") != MANIFEST_SCHEMA:
        raise ValueError("manifest schema or fields are invalid")
    source = manifest["source"]
    if not isinstance(source, dict) or set(source) != {"repository", "git_sha", "dirty"} or source.get("dirty") is not False:
        raise ValueError("manifest source is invalid")
    _validate_common(manifest["product"], source["repository"], manifest["version"], manifest["build_id"], manifest["channel"], manifest["released_at"], source["git_sha"])
    distribution = manifest["distribution"]
    signing = manifest["signing"]
    if not isinstance(signing, dict) or set(signing) != {"identity_class", "notarized", "stapled", "team_id", "notary_submissions"}:
        raise ValueError("manifest signing fields are invalid")
    if distribution == "production":
        if signing["identity_class"] != "Developer ID Application" or signing["notarized"] is not True or signing["stapled"] is not True or not TEAM_ID.fullmatch(signing["team_id"]) or set(signing["notary_submissions"]) != {"vst3"} or not NOTARY_ID.fullmatch(signing["notary_submissions"]["vst3"]):
            raise ValueError("production signing evidence is invalid")
    elif distribution == "preflight":
        if signing != {"identity_class": "ad hoc", "notarized": False, "stapled": False, "team_id": "", "notary_submissions": {}}:
            raise ValueError("preflight signing evidence is invalid")
    else:
        raise ValueError("manifest distribution is invalid")
    artifacts = manifest["artifacts"]
    if not isinstance(artifacts, list) or len(artifacts) != 1:
        raise ValueError("manifest must contain exactly one VST3 artifact")
    artifact = artifacts[0]
    if set(artifact) != {"format", "platform", "architectures", "name", "media_type", "sha256", "size_bytes"} or artifact["format"] != "vst3" or artifact["platform"] != "macos" or artifact["architectures"] != ["arm64"] or artifact["media_type"] != "application/zip":
        raise ValueError("VST3 artifact metadata is invalid")
    expected_name = f"{manifest['product']}-v{manifest['version']}-macos.vst3.zip"
    if artifact["name"] != expected_name or not SHA256.fullmatch(artifact["sha256"]) or not isinstance(artifact["size_bytes"], int) or artifact["size_bytes"] <= 0:
        raise ValueError("VST3 artifact identity or hash is invalid")
    screenshot = manifest["screenshot"]
    if set(screenshot) != {"role", "name", "media_type", "width", "height", "logical_width", "logical_height", "dpi_scale", "source_git_sha", "sha256", "size_bytes"} or screenshot["role"] != "default-ui" or screenshot["media_type"] != "image/png" or screenshot["source_git_sha"] != source["git_sha"] or screenshot["logical_width"] != screenshot["width"] or screenshot["logical_height"] != screenshot["height"] or screenshot["dpi_scale"] != 1.0 or not SCREENSHOT_NAME.fullmatch(screenshot["name"]) or not SHA256.fullmatch(screenshot["sha256"]) or not isinstance(screenshot["size_bytes"], int) or screenshot["size_bytes"] <= 0:
        raise ValueError("screenshot metadata is invalid")
    changelog = manifest["changelog"]
    if set(changelog) != {"name", "format", "media_type", "sha256", "size_bytes"} or changelog["name"] != "CHANGELOG.md" or changelog["format"] != "markdown" or changelog["media_type"] != "text/markdown; charset=utf-8" or not SHA256.fullmatch(changelog["sha256"]) or not isinstance(changelog["size_bytes"], int) or changelog["size_bytes"] <= 0:
        raise ValueError("changelog metadata is invalid")
    for name, expected_hash, expected_size in [(artifact["name"], artifact["sha256"], artifact["size_bytes"]), (screenshot["name"], screenshot["sha256"], screenshot["size_bytes"]), (changelog["name"], changelog["sha256"], changelog["size_bytes"])]:
        path = root / name
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"release file is not a regular file: {name}")
        actual_hash, actual_size = file_digest(path)
        if actual_hash != expected_hash or actual_size != expected_size:
            raise ValueError(f"manifest hash/size mismatch: {name}")
    width, height, _, _ = validate_png(root / screenshot["name"])
    if (width, height) != (screenshot["width"], screenshot["height"]) or not screenshot["name"].endswith(f"{width}x{height}.png"):
        raise ValueError("screenshot dimensions do not match its manifest")
    manifest_path = root / "release-manifest.json"
    if manifest_path.is_file() and manifest_path.read_bytes() != canonical_json(manifest):
        raise ValueError("release-manifest.json is not canonical JSON")


def _validate_schema3_manifest_file(root: Path, name: Any, expected_hash: Any, expected_size: Any) -> None:
    if not isinstance(name, str) or not SAFE_NAME.fullmatch(name):
        raise ValueError("schema 3 manifest file name is invalid")
    if not isinstance(expected_hash, str) or not SHA256.fullmatch(expected_hash) or not isinstance(expected_size, int) or isinstance(expected_size, bool) or expected_size <= 0:
        raise ValueError(f"schema 3 manifest file metadata is invalid: {name}")
    path = root / name
    _validate_regular_file(path, name)
    actual_hash, actual_size = file_digest(path)
    if actual_hash != expected_hash or actual_size != expected_size:
        raise ValueError(f"manifest hash/size mismatch: {name}")


def _validate_schema3_assets(manifest: dict[str, Any], root: Path, names: set[str]) -> None:
    source_sha = manifest["source"]["git_sha"]
    screenshot = manifest["screenshot"]
    screenshot_fields = {"role", "name", "media_type", "width", "height", "logical_width", "logical_height", "dpi_scale", "source_git_sha", "sha256", "size_bytes"}
    if (
        not isinstance(screenshot, dict)
        or set(screenshot) != screenshot_fields
        or screenshot["role"] != "default-ui"
        or not isinstance(screenshot["name"], str)
        or screenshot["name"] in names
        or screenshot["media_type"] != "image/png"
        or screenshot["source_git_sha"] != source_sha
        or screenshot["logical_width"] != screenshot["width"]
        or screenshot["logical_height"] != screenshot["height"]
        or screenshot["dpi_scale"] != 1.0
        or not SCREENSHOT_NAME.fullmatch(screenshot["name"])
        or not isinstance(screenshot["sha256"], str)
        or not SHA256.fullmatch(screenshot["sha256"])
        or not isinstance(screenshot["size_bytes"], int)
        or isinstance(screenshot["size_bytes"], bool)
        or screenshot["size_bytes"] <= 0
    ):
        raise ValueError("screenshot metadata is invalid")
    _validate_schema3_manifest_file(root, screenshot["name"], screenshot["sha256"], screenshot["size_bytes"])
    width, height, _, _ = validate_png(root / screenshot["name"])
    if (width, height) != (screenshot["width"], screenshot["height"]) or not screenshot["name"].endswith(f"{width}x{height}.png"):
        raise ValueError("screenshot dimensions do not match its manifest")
    names.add(screenshot["name"])

    changelog = manifest["changelog"]
    if (
        not isinstance(changelog, dict)
        or set(changelog) != {"name", "format", "media_type", "sha256", "size_bytes"}
        or changelog["name"] != "CHANGELOG.md"
        or changelog["name"] in names
        or changelog["format"] != "markdown"
        or changelog["media_type"] != "text/markdown; charset=utf-8"
        or not isinstance(changelog["sha256"], str)
        or not SHA256.fullmatch(changelog["sha256"])
        or not isinstance(changelog["size_bytes"], int)
        or isinstance(changelog["size_bytes"], bool)
        or changelog["size_bytes"] <= 0
    ):
        raise ValueError("changelog metadata is invalid")
    _validate_schema3_manifest_file(root, changelog["name"], changelog["sha256"], changelog["size_bytes"])
    names.add(changelog["name"])


def _validate_schema3_manifest(manifest: dict[str, Any], root: Path) -> None:
    required = {"schema_version", "product", "build_id", "version", "channel", "released_at", "source", "distribution", "artifacts", "screenshot", "changelog"}
    if set(manifest) != required or manifest.get("schema_version") != MANIFEST_SCHEMA_V3:
        raise ValueError("schema 3 manifest fields are invalid")
    source = manifest["source"]
    if not isinstance(source, dict) or set(source) != {"repository", "git_sha", "dirty"} or source.get("dirty") is not False:
        raise ValueError("manifest source is invalid")
    _validate_common(manifest["product"], source["repository"], manifest["version"], manifest["build_id"], manifest["channel"], manifest["released_at"], source["git_sha"])
    if manifest["product"] != "chromascope" or manifest["channel"] != "nightly" or manifest["distribution"] != "production":
        raise ValueError("schema 3 is only available for production Chromascope nightlies")
    expected_build_id = f"chromascope-v{manifest['version']}-{source['git_sha'][:12]}"
    if manifest["build_id"] != expected_build_id:
        raise ValueError(f"schema 3 build id must be {expected_build_id}")

    artifacts = manifest["artifacts"]
    if not isinstance(artifacts, list) or len(artifacts) != 2:
        raise ValueError("schema 3 manifest must contain exactly two VST3 artifacts")
    expected_names = {
        ("macos", "arm64"): f"chromascope-v{manifest['version']}-macos.vst3.zip",
        ("windows", "x86_64"): f"chromascope-v{manifest['version']}-windows-x86_64-unsigned.vst3.zip",
    }
    seen_targets: set[tuple[str, str]] = set()
    names: set[str] = set()
    for artifact in artifacts:
        if (
            not isinstance(artifact, dict)
            or set(artifact) != {"format", "platform", "architectures", "name", "media_type", "sha256", "size_bytes", "security"}
            or artifact["format"] != "vst3"
            or artifact["media_type"] != "application/zip"
            or not isinstance(artifact["architectures"], list)
            or len(artifact["architectures"]) != 1
            or not isinstance(artifact["platform"], str)
            or not isinstance(artifact["architectures"][0], str)
            or not isinstance(artifact["name"], str)
        ):
            raise ValueError("schema 3 artifact metadata is invalid")
        target = (artifact["platform"], artifact["architectures"][0])
        if target not in expected_names or target in seen_targets or artifact["name"] != expected_names[target] or artifact["name"] in names:
            raise ValueError("schema 3 artifact identity is invalid")
        if target == ("macos", "arm64"):
            security = artifact["security"]
            if (
                not isinstance(security, dict)
                or set(security) != {"status", "certificate", "team_id", "notarized", "stapled", "notary_submission"}
                or security["status"] != "signed"
                or security["certificate"] != "Developer ID Application"
                or security["team_id"] != CHROMASCOPE_TEAM_ID
                or security["notarized"] is not True
                or security["stapled"] is not True
                or not isinstance(security["notary_submission"], str)
                or not NOTARY_ID.fullmatch(security["notary_submission"])
            ):
                raise ValueError("schema 3 artifact security is invalid")
        elif artifact["security"] != {"status": "unsigned", "certificate": None}:
            raise ValueError("schema 3 artifact security is invalid")
        _validate_schema3_manifest_file(root, artifact["name"], artifact["sha256"], artifact["size_bytes"])
        seen_targets.add(target)
        names.add(artifact["name"])
    if seen_targets != set(expected_names):
        raise ValueError("schema 3 artifact targets are incomplete")
    _validate_schema3_assets(manifest, root, names)
    manifest_path = root / "release-manifest.json"
    if manifest_path.is_file() and manifest_path.read_bytes() != canonical_json(manifest):
        raise ValueError("release-manifest.json is not canonical JSON")


def validate_manifest(manifest: dict[str, Any], root: Path) -> None:
    if not isinstance(manifest, dict):
        raise ValueError("manifest must be an object")
    if manifest.get("schema_version") == MANIFEST_SCHEMA_V2:
        _validate_schema2_manifest(manifest, root)
    elif manifest.get("schema_version") == MANIFEST_SCHEMA_V3:
        _validate_schema3_manifest(manifest, root)
    else:
        raise ValueError("manifest schema or fields are invalid")


def validate_canonical_source(manifest: dict[str, Any], repo_root: Path) -> None:
    def git(*args: str) -> str:
        result = subprocess.run(["git", *args], cwd=repo_root, text=True, capture_output=True, check=False)
        if result.returncode:
            raise ValueError(f"git {' '.join(args)} failed")
        return result.stdout.strip()
    if git("symbolic-ref", "--quiet", "--short", "HEAD") != "main":
        raise ValueError("production release source must be a non-detached main checkout")
    if git("status", "--porcelain", "--untracked-files=all"):
        raise ValueError("production release source must be clean")
    git("fetch", "origin", "main", "--quiet")
    head = git("rev-parse", "HEAD")
    origin = git("rev-parse", "refs/remotes/origin/main")
    source_sha = manifest["source"]["git_sha"]
    if head != origin or head != source_sha:
        raise ValueError("production release source must match HEAD, origin/main, and manifest source SHA")


def _request(url: str, method: str, body: bytes | None, headers: dict[str, str]) -> tuple[int, bytes]:
    request = Request(url, method=method, data=body, headers=headers)
    try:
        with urlopen(request, timeout=60) as response:
            return response.status, response.read()
    except (HTTPError, URLError) as error:
        detail = error.read().decode("utf-8", "replace")[:400] if isinstance(error, HTTPError) else str(error)
        raise RuntimeError(f"{method} {url} failed: {detail}") from error


def publish_release(*, endpoint: str, token: str, manifest: dict[str, Any], root: Path, repo_root: Optional[Path] = None, transport: Callable[[str, str, bytes | None, dict[str, str]], tuple[int, bytes]] = _request) -> None:
    if endpoint != PRODUCTION_ORIGIN:
        raise ValueError("production publishing requires the exact PortalSurfer origin")
    if not token:
        raise ValueError("PORTALSURFER_RELEASE_TOKEN is required for publishing")
    if manifest.get("schema_version") != MANIFEST_SCHEMA_V2:
        raise ValueError("the Python publisher supports schema 2 manifests only")
    if manifest.get("distribution") != "production":
        raise ValueError("only production manifests may be published")
    validate_manifest(manifest, root)
    validate_canonical_source(manifest, repo_root or root.parents[2])
    product = manifest["product"]
    status, payload = transport(f"{endpoint}/plugins/api/v1/products/{product}/releases", "GET", None, {"Accept": "application/json"})
    if not 200 <= status < 300:
        raise RuntimeError(f"PortalSurfer capability check failed ({status}); no files were uploaded")
    capability = json.loads(payload)
    if MANIFEST_SCHEMA not in capability.get("release_upload", {}).get("manifest_schema_versions", []):
        raise RuntimeError("PortalSurfer does not support manifest schema 2; no files were uploaded")
    base = f"{endpoint}/plugins/api/v1/products/{product}/release-uploads/{manifest['build_id']}"
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/octet-stream", "X-PortalSurfer-Release-Version": manifest["version"], "X-PortalSurfer-Release-Channel": manifest["channel"], "X-PortalSurfer-Released-At": manifest["released_at"]}
    names = [manifest["artifacts"][0]["name"], manifest["screenshot"]["name"], manifest["changelog"]["name"]]
    for name in names:
        path = root / name
        data = path.read_bytes()
        expected = manifest["artifacts"][0]["sha256"] if name == manifest["artifacts"][0]["name"] else manifest["screenshot"]["sha256"] if name == manifest["screenshot"]["name"] else manifest["changelog"]["sha256"]
        digest, size = file_digest(path)
        if digest != expected or size != len(data):
            raise ValueError(f"local file changed after manifest validation: {name}")
        transport(f"{base}/staging/files/{name}", "PUT", data, {**headers, "Content-Length": str(size), "X-PortalSurfer-Sha256": digest})
    body = canonical_json(manifest)
    transport(f"{base}/commit", "PUT", body, {"Authorization": f"Bearer {token}", "Content-Type": MANIFEST_CONTENT_TYPE, "Content-Length": str(len(body)), "X-PortalSurfer-Manifest-Sha256": hashlib.sha256(body).hexdigest(), "X-PortalSurfer-Release-Version": manifest["version"], "X-PortalSurfer-Release-Channel": manifest["channel"], "X-PortalSurfer-Released-At": manifest["released_at"]})


if __name__ == "__main__":
    print("This helper is imported by scripts/release.sh; it does not publish by itself.", file=__import__("sys").stderr)
    raise SystemExit(2)
