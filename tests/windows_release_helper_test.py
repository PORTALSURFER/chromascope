#!/usr/bin/env python3
"""Deterministic regression tests for the unsigned Windows release helper."""

from __future__ import annotations

import json
import struct
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path


ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import windows_release_helper


SOURCE_SHA = "a" * 40
TOYBOX_REVISION = "b" * 40
RADIANT_REVISION = "c" * 40
SDK_REVISION = "d" * 40


def pe_image(
    machine: int = windows_release_helper.IMAGE_FILE_MACHINE_AMD64,
    optional_magic: int = windows_release_helper.PE32_PLUS_MAGIC,
    certificate: bool = False,
) -> bytes:
    data = bytearray(0x200)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\x00\x00"
    struct.pack_into("<H", data, 0x84, machine)
    struct.pack_into("<H", data, 0x94, 0xF0)
    struct.pack_into("<H", data, 0x98, optional_magic)
    struct.pack_into("<I", data, 0x98 + 108, 5 if certificate else 0)
    if certificate:
        struct.pack_into("<II", data, 0x98 + 112 + (4 * 8), 0x300, 0x20)
    data[0x1F0:] = b"Chromascope Windows VST3 test payload"
    return bytes(data)


def cargo_lock() -> str:
    return f'''version = 4

[[package]]
name = "toybox"
version = "0.1.0"
source = "git+https://github.com/PORTALSURFER/toybox.git?rev={TOYBOX_REVISION}#{TOYBOX_REVISION}"

[[package]]
name = "radiant"
version = "0.1.0"
source = "git+https://github.com/PORTALSURFER/radiant.git?rev={RADIANT_REVISION}#{RADIANT_REVISION}"
'''


class WindowsReleaseHelperTests(unittest.TestCase):
    def package(self, root: Path, *, binary_data: bytes | None = None, output_name: str = "release") -> tuple[dict, Path, Path]:
        binary = root / "dist" / "Chromascope-v0.1.0.vst3" / "Contents" / "x86_64-win" / "Chromascope-v0.1.0.vst3"
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_bytes(binary_data or pe_image())
        lockfile = root / "Cargo.lock"
        lockfile.write_text(cargo_lock(), encoding="utf-8")
        output = root / output_name
        dependencies = windows_release_helper.dependency_revisions(lockfile, vst3_sdk_revision=SDK_REVISION)
        manifest = windows_release_helper.package_windows_vst3(
            binary=binary,
            output_dir=output,
            package_version="0.1.0",
            publication_version="0.1.0-nightly.7",
            channel="nightly",
            build_id="chromascope-v0.1.0-nightly.7-windows-a1b2c3d4e5f6",
            released_at="2026-09-02T10:20:30Z",
            source_sha=SOURCE_SHA,
            dependencies=dependencies,
        )
        return manifest, output, lockfile

    def test_package_emits_requested_names_layout_and_unsigned_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest, output, lockfile = self.package(Path(directory))
            archive_path = output / "chromascope-v0.1.0-nightly.7-windows-x86_64-unsigned.vst3.zip"
            manifest_path = output / "windows-artifact-manifest.json"

            self.assertEqual(sorted(path.name for path in output.iterdir()), [archive_path.name, manifest_path.name])
            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(manifest["signing_status"], "unsigned")
            self.assertIsNone(manifest["signing_certificate"])
            self.assertEqual(manifest["source"]["git_sha"], SOURCE_SHA)
            self.assertEqual(manifest["dependencies"]["toybox"]["revision"], TOYBOX_REVISION)
            self.assertEqual(manifest["dependencies"]["radiant"]["revision"], RADIANT_REVISION)
            self.assertEqual(manifest["dependencies"]["vst3sdk"]["revision"], SDK_REVISION)
            self.assertEqual(
                manifest["archive"]["layout"],
                {
                    "bundle": "Chromascope-v0.1.0.vst3",
                    "binary": "Chromascope-v0.1.0.vst3/Contents/x86_64-win/Chromascope-v0.1.0.vst3",
                },
            )
            with zipfile.ZipFile(archive_path) as archive:
                self.assertEqual(archive.namelist(), [manifest["archive"]["layout"]["binary"]])
            loaded = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(loaded, manifest)
            windows_release_helper.validate_manifest(
                loaded,
                output,
                cargo_lock=lockfile,
                vst3_sdk_revision=SDK_REVISION,
            )

    def test_package_and_manifest_are_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first, first_output, _ = self.package(root, output_name="first")
            second, second_output, _ = self.package(root, output_name="second")
            self.assertEqual(first, second)
            self.assertEqual(
                (first_output / "chromascope-v0.1.0-nightly.7-windows-x86_64-unsigned.vst3.zip").read_bytes(),
                (second_output / "chromascope-v0.1.0-nightly.7-windows-x86_64-unsigned.vst3.zip").read_bytes(),
            )
            self.assertEqual(
                (first_output / "windows-artifact-manifest.json").read_bytes(),
                (second_output / "windows-artifact-manifest.json").read_bytes(),
            )

    def test_non_x86_64_binary_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "x86_64"):
                self.package(Path(directory), binary_data=pe_image(machine=0x14C))

    def test_signed_binary_and_wrong_source_path_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, "Authenticode"):
                self.package(root, binary_data=pe_image(certificate=True))

            lockfile = root / "Cargo.lock"
            lockfile.write_text(cargo_lock(), encoding="utf-8")
            binary = root / "dist" / "wrong.vst3"
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_bytes(pe_image())
            with self.assertRaisesRegex(ValueError, "must be under"):
                windows_release_helper.package_windows_vst3(
                    binary=binary,
                    output_dir=root / "wrong-source",
                    package_version="0.1.0",
                    publication_version="0.1.0-nightly.7",
                    channel="nightly",
                    build_id="chromascope-v0.1.0-nightly.7-windows-a1b2c3d4e5f6",
                    released_at="2026-09-02T10:20:30Z",
                    source_sha=SOURCE_SHA,
                    dependencies=windows_release_helper.dependency_revisions(lockfile, vst3_sdk_revision=SDK_REVISION),
                )

    def test_archive_rejects_path_traversal_and_extra_members(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "chromascope-v0.1.0-nightly.7-windows-x86_64-unsigned.vst3.zip"
            with zipfile.ZipFile(archive, "w") as zip_file:
                zip_file.writestr("../escape.vst3", pe_image())
            with self.assertRaisesRegex(ValueError, "unsafe"):
                windows_release_helper.validate_archive(
                    archive,
                    package_version="0.1.0",
                    publication_version="0.1.0-nightly.7",
                )

            with zipfile.ZipFile(archive, "w") as zip_file:
                zip_file.writestr(windows_release_helper.bundle_member_name("0.1.0"), pe_image())
                zip_file.writestr("extra.txt", b"unexpected")
            with self.assertRaisesRegex(ValueError, "exactly one"):
                windows_release_helper.validate_archive(
                    archive,
                    package_version="0.1.0",
                    publication_version="0.1.0-nightly.7",
                )

    def test_manifest_rejects_archive_hash_or_signing_tampering(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest, output, lockfile = self.package(Path(directory))
            archive = output / manifest["archive"]["name"]
            archive.write_bytes(archive.read_bytes() + b"tampered")
            with self.assertRaises(ValueError):
                windows_release_helper.validate_manifest(
                    manifest,
                    output,
                    cargo_lock=lockfile,
                    vst3_sdk_revision=SDK_REVISION,
                )

            manifest, output, lockfile = self.package(Path(directory), output_name="signed-tamper")
            manifest["signing_status"] = "signed"
            with self.assertRaisesRegex(ValueError, "unsigned"):
                windows_release_helper.validate_manifest(
                    manifest,
                    output,
                    cargo_lock=lockfile,
                    vst3_sdk_revision=SDK_REVISION,
                )

    def test_malformed_archive_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "chromascope-v0.1.0-nightly.7-windows-x86_64-unsigned.vst3.zip"
            archive.write_bytes(b"not a zip")
            with self.assertRaisesRegex(ValueError, "valid ZIP"):
                windows_release_helper.validate_archive(
                    archive,
                    package_version="0.1.0",
                    publication_version="0.1.0-nightly.7",
                )


if __name__ == "__main__":
    unittest.main()
