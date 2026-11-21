# 0002. One Cargo workspace for client, server and shared crates

- Status: Accepted
- Date: 2026-11-21

## Context

Protocol types, permissions and validation are shared between client and server.

## Decision

One workspace (edition 2024, resolver 3, MSRV = stable − 2) with `crates/`, `apps/` and `xtask/`. Workspace lints forbid `unsafe_code`; a future audited FFI module in `duydas-voice` will be the only exception.

## Consequences

Shared code cannot drift. CI builds everything on every change.
