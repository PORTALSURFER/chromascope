#!/usr/bin/env python3
"""Focused regression tests for the Chromascope release producer."""

from __future__ import annotations

import struct
import sys
import tempfile
import unittest
import zlib
from pathlib import Path


ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import release_helper


TEAM_ID = "TEAM123456"
NOTARY_ID = "12345678-1234-4123-8123-123456789abc"


def png_1x1() -> bytes:
    def chunk(kind: bytes, payload: bytes) -> bytes:
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)

    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)) + chunk(b"IDAT", b"x") + chunk(b"IEND", b"")


class ReleaseHelperTests(unittest.TestCase):
    def test_publication_version_derivation(self) -> None:
        self.assertEqual(release_helper.derive_publication_version("1.2.3", "stable", 17), "1.2.3")
        self.assertEqual(release_helper.derive_publication_version("1.2.3", "rc", 17), "1.2.3-rc.17")
        self.assertEqual(release_helper.derive_publication_version("1.2.3", "nightly", 17), "1.2.3-nightly.17")
        for sequence in (0, "0", -1, "not-a-sequence"):
            with self.subTest(sequence=sequence), self.assertRaises(ValueError):
                release_helper.derive_publication_version("1.2.3", "nightly", sequence)

    def test_publication_version_validation(self) -> None:
        release_helper.validate_publication_version("1.2.3", "1.2.3-nightly.17", "nightly")
        for publication_version in ("1.2.4-nightly.17", "1.2.3-nightly.0", "1.2.3"):
            with self.subTest(publication_version=publication_version), self.assertRaises(ValueError):
                release_helper.validate_publication_version("1.2.3", publication_version, "nightly")

    def test_manifest_requires_channel_qualified_nightly_publication(self) -> None:
        with self.assertRaisesRegex(ValueError, "nightly release version syntax"):
            release_helper.build_manifest(
                product="chromascope",
                repository="PORTALSURFER/chromascope",
                version="1.0.0",
                build_id="chromascope-v1.0.0-test",
                channel="nightly",
                released_at="2026-08-30T00:00:00Z",
                git_sha="a" * 40,
                vst3=Path("missing.zip"),
                screenshot=Path("missing.png"),
                changelog=Path("missing.md"),
                distribution="preflight",
            )

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            publication_version = "1.0.0-nightly.17"
            vst3 = root / f"chromascope-v{publication_version}-macos.vst3.zip"
            screenshot = root / "chromascope-default-1x1.png"
            changelog = root / "CHANGELOG.md"
            vst3.write_bytes(b"vst3 zip")
            screenshot.write_bytes(png_1x1())
            changelog.write_text("release notes\n", encoding="utf-8")
            manifest = release_helper.build_manifest(
                product="chromascope",
                repository="PORTALSURFER/chromascope",
                version=publication_version,
                build_id=f"chromascope-v{publication_version}-test",
                channel="nightly",
                released_at="2026-08-30T00:00:00Z",
                git_sha="a" * 40,
                vst3=vst3,
                screenshot=screenshot,
                changelog=changelog,
                distribution="preflight",
            )
            release_helper.validate_manifest(manifest, root)
            self.assertEqual(manifest["version"], publication_version)
            self.assertEqual(manifest["artifacts"][0]["name"], vst3.name)

            manifest["version"] = "1.0.0"
            with self.assertRaisesRegex(ValueError, "nightly release version syntax"):
                release_helper.validate_manifest(manifest, root)

    def test_release_script_captures_and_passes_valid_team_id(self) -> None:
        script = (ROOT / "scripts" / "release.sh").read_text(encoding="utf-8")
        staple = '  xcrun stapler staple "${bundle}" >/dev/null'
        capture = '  signing_team_id="$(codesign -dv --verbose=4 "${bundle}" 2>&1 | sed -n \'s/^TeamIdentifier=//p\' | head -n 1)"'

        self.assertIn(capture, script)
        self.assertIn('  [[ "${signing_team_id}" =~ ^[A-Z0-9]{10}$ ]] ||', script)
        capture_index = script.index(capture)
        for gate in (
            '  codesign --force --deep --timestamp',
            '  codesign --verify --deep --strict "${bundle}"',
            '  xcrun notarytool submit',
            staple,
            '  xcrun stapler validate "${bundle}" >/dev/null',
            '  codesign -vvvv -R=notarized --check-notarization "${bundle}" >/dev/null',
        ):
            self.assertLess(script.index(gate), capture_index, gate)
        self.assertLess(capture_index, script.index('else\n  codesign --force --deep --sign - "${bundle}"'))
        self.assertIn('"${distribution}" "${signing_team_id}" "${vst3_notary_id}"', script)
        self.assertIn("signing_team_id=team_id,", script)

    def test_production_manifest_requires_valid_team_and_notary_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            vst3 = root / "chromascope-v1.0.0-macos.vst3.zip"
            screenshot = root / "chromascope-default-1x1.png"
            changelog = root / "CHANGELOG.md"
            vst3.write_bytes(b"vst3 zip")
            screenshot.write_bytes(png_1x1())
            changelog.write_text("release notes\n", encoding="utf-8")

            def build(team_id: str, notary_id: str) -> dict:
                return release_helper.build_manifest(
                    product="chromascope",
                    repository="PORTALSURFER/chromascope",
                    version="1.0.0",
                    build_id="chromascope-v1.0.0-test",
                    channel="stable",
                    released_at="2026-08-30T00:00:00Z",
                    git_sha="a" * 40,
                    vst3=vst3,
                    screenshot=screenshot,
                    changelog=changelog,
                    distribution="production",
                    signing_team_id=team_id,
                    vst3_notary_id=notary_id,
                )

            manifest = build(TEAM_ID, NOTARY_ID)
            self.assertEqual(manifest["signing"]["team_id"], TEAM_ID)
            self.assertEqual(manifest["signing"]["notary_submissions"], {"vst3": NOTARY_ID})

            for team_id, notary_id in (
                ("", NOTARY_ID),
                ("TEAM12345", NOTARY_ID),
                ("team123456", NOTARY_ID),
                (TEAM_ID, ""),
                (TEAM_ID, "not-a-notary-id"),
            ):
                with self.subTest(team_id=team_id, notary_id=notary_id), self.assertRaisesRegex(
                    ValueError, "production signing/notarization evidence is incomplete"
                ):
                    build(team_id, notary_id)


if __name__ == "__main__":
    unittest.main()
