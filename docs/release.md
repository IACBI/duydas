# Release process

Status: **planned.** Nothing is released yet. This page records the agreed design so the first release does not improvise.

## Channels and formats

| Platform | Package | Updates | Signing |
|---|---|---|---|
| Windows 10+ | Per-user installer (no admin) via Velopack, full packages | In-app, verified (below) | Authenticode via SignPath Foundation (OSS program, subject to acceptance) |
| macOS 13+ | Signed app bundle via Velopack | In-app, verified | Developer ID + notarization once an Apple Developer account exists; until then macOS builds are for development only |
| Linux | Flatpak on Flathub | Flathub | Flathub signing |

Delta updates are deferred until full-package updates are proven.

## Update trust (ADR 0010)

The client accepts an update only if signed metadata binds **version, channel, platform, architecture, SHA-256 digest and size**, has not expired, and is not older than the installed version. The downloaded file is verified against that metadata again at installer handoff. The metadata root key is offline; rotation and compromise recovery are documented here before the first signed release. Before the first signed release, tests will cover tampered files, stale metadata, wrong platform, channel or architecture, interrupted downloads and key rotation. Any library shipped in the updater must satisfy ADR 0021.

## Pipeline

1. Tag `vX.Y.Z` on `main` after CI is green and `CHANGELOG.md` is updated.
2. The release workflow builds on GitHub-hosted runners. Signing runs only in the protected `release` environment, which requires maintainer approval, and never runs for pull requests.
3. Artifacts get build-provenance attestations and a CycloneDX SBOM.
4. Smoke test on clean machines (standard user, quarantined download on macOS) before publishing the update metadata.
