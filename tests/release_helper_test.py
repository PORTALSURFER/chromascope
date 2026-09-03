#!/usr/bin/env python3
"""Focused regression tests for the Chromascope release producer."""

from __future__ import annotations

import re
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


def schema3_fixture(root: Path) -> tuple[dict, Path, Path, Path, Path]:
    publication_version = "1.0.0-nightly.17"
    source_sha = "a1b2c3d4e5f6" + "a" * 28
    macos = root / f"chromascope-v{publication_version}-macos.vst3.zip"
    windows = root / f"chromascope-v{publication_version}-windows-x86_64-unsigned.vst3.zip"
    screenshot = root / "chromascope-default-1x1.png"
    changelog = root / "CHANGELOG.md"
    macos.write_bytes(b"signed macOS archive")
    windows.write_bytes(b"unsigned Windows archive")
    screenshot.write_bytes(png_1x1())
    changelog.write_text("nightly release notes\n", encoding="utf-8")
    manifest = release_helper.build_manifest(
        product="chromascope",
        repository="PORTALSURFER/chromascope",
        version=publication_version,
        build_id=f"chromascope-v{publication_version}-{source_sha[:12]}",
        channel="nightly",
        released_at="2026-09-02T10:20:30Z",
        git_sha=source_sha,
        vst3=macos,
        windows_vst3=windows,
        screenshot=screenshot,
        changelog=changelog,
        distribution="production",
        signing_team_id=release_helper.CHROMASCOPE_TEAM_ID,
        vst3_notary_id=NOTARY_ID,
    )
    (root / "release-manifest.json").write_bytes(release_helper.canonical_json(manifest))
    return manifest, macos, windows, screenshot, changelog


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

    def test_schema2_stable_manifest_shape_remains_mac_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            vst3 = root / "chromascope-v1.0.0-macos.vst3.zip"
            screenshot = root / "chromascope-default-1x1.png"
            changelog = root / "CHANGELOG.md"
            vst3.write_bytes(b"vst3 zip")
            screenshot.write_bytes(png_1x1())
            changelog.write_text("stable release\n", encoding="utf-8")
            manifest = release_helper.build_manifest(
                product="chromascope",
                repository="PORTALSURFER/chromascope",
                version="1.0.0",
                build_id="chromascope-v1.0.0-test",
                channel="stable",
                released_at="2026-09-02T10:20:30Z",
                git_sha="a" * 40,
                vst3=vst3,
                screenshot=screenshot,
                changelog=changelog,
                distribution="production",
                signing_team_id=TEAM_ID,
                vst3_notary_id=NOTARY_ID,
            )
            self.assertEqual(
                set(manifest),
                {"schema_version", "product", "build_id", "version", "channel", "released_at", "source", "distribution", "signing", "artifacts", "screenshot", "changelog"},
            )
            self.assertEqual(manifest["schema_version"], release_helper.MANIFEST_SCHEMA_V2)
            self.assertEqual(len(manifest["artifacts"]), 1)
            self.assertNotIn("security", manifest["artifacts"][0])
            release_helper.validate_manifest(manifest, root)

    def test_schema3_combines_artifacts_with_hashes_and_security(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest, macos, windows, _, _ = schema3_fixture(root)
            self.assertEqual(
                set(manifest),
                {"schema_version", "product", "build_id", "version", "channel", "released_at", "source", "distribution", "artifacts", "screenshot", "changelog"},
            )
            self.assertNotIn("signing", manifest)
            self.assertEqual(manifest["schema_version"], release_helper.MANIFEST_SCHEMA_V3)
            self.assertEqual(manifest["build_id"], f"chromascope-v{manifest['version']}-{manifest['source']['git_sha'][:12]}")
            self.assertEqual(manifest["source"]["git_sha"], "a1b2c3d4e5f6" + "a" * 28)
            self.assertEqual(manifest["artifacts"][0]["name"], macos.name)
            self.assertEqual(manifest["artifacts"][0]["platform"], "macos")
            self.assertEqual(manifest["artifacts"][0]["architectures"], ["arm64"])
            self.assertEqual(manifest["artifacts"][0]["security"], {
                "status": "signed",
                "certificate": "Developer ID Application",
                "team_id": release_helper.CHROMASCOPE_TEAM_ID,
                "notarized": True,
                "stapled": True,
                "notary_submission": NOTARY_ID,
            })
            self.assertEqual(manifest["artifacts"][1]["name"], windows.name)
            self.assertEqual(manifest["artifacts"][1]["platform"], "windows")
            self.assertEqual(manifest["artifacts"][1]["architectures"], ["x86_64"])
            self.assertEqual(manifest["artifacts"][1]["security"], {"status": "unsigned", "certificate": None})
            for artifact, path in zip(manifest["artifacts"], (macos, windows)):
                digest, size = release_helper.file_digest(path)
                self.assertEqual((artifact["sha256"], artifact["size_bytes"]), (digest, size))
            release_helper.validate_manifest(manifest, root)

            tampered = dict(manifest)
            tampered["artifacts"] = [dict(artifact) for artifact in manifest["artifacts"]]
            tampered["artifacts"][1]["sha256"] = "0" * 64
            with self.assertRaisesRegex(ValueError, "hash/size mismatch"):
                release_helper.validate_manifest(tampered, root)

    def test_schema3_requires_windows_and_exact_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            publication_version = "1.0.0-nightly.17"
            macos = root / f"chromascope-v{publication_version}-macos.vst3.zip"
            screenshot = root / "chromascope-default-1x1.png"
            changelog = root / "CHANGELOG.md"
            macos.write_bytes(b"signed macOS archive")
            screenshot.write_bytes(png_1x1())
            changelog.write_text("nightly release notes\n", encoding="utf-8")
            common = dict(
                product="chromascope",
                repository="PORTALSURFER/chromascope",
                version=publication_version,
                channel="nightly",
                released_at="2026-09-02T10:20:30Z",
                git_sha="a1b2c3d4e5f6" + "a" * 28,
                vst3=macos,
                screenshot=screenshot,
                changelog=changelog,
                distribution="production",
                signing_team_id=release_helper.CHROMASCOPE_TEAM_ID,
                vst3_notary_id=NOTARY_ID,
            )
            with self.assertRaisesRegex(ValueError, "requires the Windows artifact"):
                release_helper.build_manifest(
                    build_id=f"chromascope-v{publication_version}-{common['git_sha'][:12]}",
                    **common,
                )
            _, _, windows, _, _ = schema3_fixture(root)
            with self.assertRaisesRegex(ValueError, "schema 3 build id"):
                release_helper.build_manifest(
                    build_id="chromascope-v1.0.0-nightly.17-wrong",
                    windows_vst3=windows,
                    **common,
                )
            with self.assertRaisesRegex(ValueError, "Apple team ID"):
                release_helper.build_manifest(
                    build_id=f"chromascope-v{publication_version}-{common['git_sha'][:12]}",
                    windows_vst3=windows,
                    signing_team_id=TEAM_ID,
                    **{key: value for key, value in common.items() if key != "signing_team_id"},
                )

    def test_release_contract_pins_combined_workflow_and_publisher_transport(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        script = (ROOT / "scripts" / "release.sh").read_text(encoding="utf-8")
        self.assertIn("prepare:", workflow)
        self.assertIn("uses: ./.github/workflows/windows-release.yml", workflow)
        self.assertIn("actions/download-artifact@", workflow)
        self.assertIn("scripts/windows_release_helper.py validate", workflow)
        self.assertIn("id-token: write", workflow)
        self.assertEqual(workflow.count("id-token: write"), 1)
        self.assertIn("165776d6707ab6d9e8bb76b2a8866654140ca6bc", workflow)
        for shared_field in ("source_sha", "package_version", "publication_version", "channel", "build_id", "released_at"):
            self.assertIn(shared_field, workflow)
        self.assertIn("--windows-release-dir", script)
        self.assertIn("--publisher-script", script)
        self.assertIn('node "${publisher_script}"', script)
        self.assertNotIn("--token", script)

    def test_release_preflight_executes_pr_safe_combined_nightly_chain(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "release-preflight.yml").read_text(encoding="utf-8")
        production_workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        harness = (ROOT / "tests" / "release_pipeline_integration.py").read_text(encoding="utf-8")

        self.assertNotIn("pull_request_target", workflow)
        self.assertNotIn("secrets", workflow)
        self.assertNotIn("environment: production", workflow)
        self.assertNotIn("contents: write", workflow)
        self.assertNotIn("id-token: write", workflow)
        for action in re.findall(r"(?m)^\s+uses:\s+([^\s#]+)", workflow):
            if not action.startswith("./"):
                self.assertRegex(action, r"@[0-9a-f]{40}\Z", action)

        self.assertIn("github.event.pull_request.head.sha", workflow)
        self.assertIn("github.run_attempt", workflow)
        self.assertIn("attempt_suffix", workflow)

        windows_lane = workflow.split("\n  windows_integration:\n", 1)[1].split("\n  integration:\n", 1)[0]
        integration_lane = workflow.split("\n  integration:\n", 1)[1]
        self.assertIn("uses: ./.github/workflows/windows-release.yml", windows_lane)
        self.assertIn("permissions:\n      contents: read", windows_lane)
        self.assertNotRegex(windows_lane, r"(?m)^\s*(?:environment|secrets):")
        for field in ("source_sha", "package_version", "publication_version", "build_id", "released_at"):
            self.assertIn(f"      {field}: ${{{{ needs.prepare.outputs.{field} }}}}", windows_lane)
        self.assertIn("      channel: nightly", windows_lane)

        self.assertIn("needs: [prepare, preflight, windows_integration]", integration_lane)
        self.assertNotRegex(integration_lane, r"(?m)^\s*secrets(?:\.|:)")
        self.assertNotRegex(integration_lane, r"(?m)^\s*environment:")
        self.assertNotRegex(integration_lane, r"(?m)^\s*(?:contents|actions|id-token):\s*write\b")
        self.assertEqual(integration_lane.count("actions/download-artifact@"), 2)
        self.assertIn(
            "name: chromascope-macos-preflight-${{ needs.prepare.outputs.build_id }}-${{ needs.prepare.outputs.attempt_suffix }}",
            integration_lane,
        )
        self.assertIn("name: chromascope-windows-${{ needs.prepare.outputs.build_id }}", integration_lane)
        for forbidden in ("token:", "repository:", "run-id:", "pattern:", "merge-multiple:"):
            self.assertNotIn(forbidden, integration_lane)
        self.assertIn("tests/release_pipeline_integration.py", workflow)

        def publisher_pin(text: str) -> str:
            match = re.search(r"(?m)^\s*PUBLISHER_COMMIT:\s*([0-9a-f]{40})\s*$", text)
            self.assertIsNotNone(match)
            return match.group(1)  # type: ignore[union-attr]

        self.assertEqual(publisher_pin(workflow), publisher_pin(production_workflow))
        self.assertEqual(publisher_pin(workflow), "165776d6707ab6d9e8bb76b2a8866654140ca6bc")
        for argument in (
            "--macos-artifact-root",
            "--windows-artifact-root",
            "--publisher-script",
            "--package-version",
            "--publication-version",
            "--build-id",
            "--source-sha",
            "--released-at",
        ):
            self.assertIn(argument, harness)
        for contract in (
            "windows_release_helper.validate_manifest(",
            "release_helper.build_manifest(",
            "release_helper.canonical_json(",
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            '"127.0.0.1"',
            "TEST_ATTESTATION_TOKEN",
            "api_mock.commit_count == 1",
        ):
            self.assertIn(contract, harness)

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
