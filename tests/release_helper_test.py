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
