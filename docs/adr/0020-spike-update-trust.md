# 0020. Spike: Velopack + update metadata verification

- Status: Pending
- Date: 2026-11-21

## Context

ADR 0010 requires verification at installer handoff, which depends on how Velopack applies packages.

## Decision

Dry run on Windows: package with Velopack, sign metadata, verify in-app with a TUF client, tamper with artifact and metadata and confirm rejection.

## Consequences

Confirms or replaces the Velopack choice before Phase 6.
