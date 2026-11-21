# 0010. Velopack packages, Flatpak, and TUF-style update metadata

- Status: Accepted
- Date: 2026-11-21

## Context

The updater runs code on users' machines; a signed feed alone does not bind the installed bytes.

## Decision

Windows and macOS use Velopack full packages; Linux ships through Flathub. Updates require signed metadata binding version, channel, platform, architecture, digest and size, with expiry, downgrade protection and an offline root key (candidate implementation: `tough`). The artifact is re-verified at installer handoff. Delta updates are deferred.

## Consequences

Update compromise requires the offline key, not just the hosting account.
