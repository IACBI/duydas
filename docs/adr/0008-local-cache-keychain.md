# 0008. SQLite cache, tokens in the OS keychain

- Status: Accepted
- Date: 2026-11-21

## Context

The client needs offline history and fast startup without storing secrets in plain files.

## Decision

rusqlite (bundled) for cached history and settings. Session tokens live only in the OS keychain (`keyring`), with memory-only sessions when no keychain is available. Retention and logout-wipe policy is defined in Phase 1; cache encryption is decided before E2EE ships.

## Consequences

Stolen-file exposure is limited to cached content, which the policy bounds.
