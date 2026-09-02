# Windows release artifact

`.github/workflows/windows-release.yml` is the independent Windows packaging
lane. It is manually dispatched, checks out `main`, and builds only the
x86_64-pc-windows-msvc VST3 target. The workflow pins the Windows Server
generation to `windows-2022` and Rust to `1.97.1`; each manifest also records
the concrete hosted-image and compiler details used for that build. The
workflow never signs the binary, uses no certificate, uploads no catalog entry,
and does not create or attach a GitHub Release.

The Windows build emits the bundle under the repository's `dist/` directory:

```text
dist/Chromascope-v<package-version>.vst3/Contents/x86_64-win/Chromascope-v<package-version>.vst3
```

The packaging helper validates that exact source path and its AMD64 PE32+
binary, then emits these two files in the Actions artifact directory:

```text
chromascope-v<publication-version>-windows-x86_64-unsigned.vst3.zip
windows-artifact-manifest.json
```

The ZIP contains exactly this one member, using forward-slash paths:

```text
Chromascope-v<package-version>.vst3/Contents/x86_64-win/Chromascope-v<package-version>.vst3
```

The adjacent schema-1 manifest records the package and publication versions,
channel, immutable source SHA, the `Cargo.lock` revisions for Toybox and
Radiant, the pinned VST3 SDK revision, the exact archive layout, member/archive
SHA-256 hashes and sizes, build-environment provenance, and:

```json
{
  "build_environment": {
    "runner": {
      "image": "windows-2022",
      "image_os": "win22",
      "image_version": "<YYYYMMDD.build.patch>"
    },
    "rust": {
      "toolchain": "1.97.1",
      "target": "x86_64-pc-windows-msvc",
      "rustc_version": "rustc 1.97.1 (...)"
    },
    "python": {
      "implementation": "CPython",
      "version": "<major.minor.patch>"
    }
  }
}
```

It also records:

```json
{
  "signing_status": "unsigned",
  "signing_certificate": null
}
```

`scripts/windows_release_helper.py` rejects malformed ZIPs, duplicate or
extra members, directory entries, absolute/drive-qualified/backslash/traversal
paths, symlinks and special files, encrypted or unsupported compression, bad
PE headers, non-x86_64 binaries, stale hashes, and mismatched dependency
revisions or build-environment provenance. The helper writes canonical JSON and
uses fixed ZIP timestamps so the same inputs, including the recorded environment,
produce the same archive and manifest.

For a local Windows build with the pinned SDK available:

```text
cargo build --locked --release --target x86_64-pc-windows-msvc --features vst3
python scripts/windows_release_helper.py package ...
python scripts/windows_release_helper.py validate ...
```

The exact command-line arguments are visible in the workflow. The helper tests
run with:

```text
python3 -m unittest discover -s tests -p '*_test.py'
```

The signed/notarized macOS release workflow remains the production publication
path and is intentionally separate from this unsigned Windows inspection
artifact.
