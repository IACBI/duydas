# 0011. No telemetry by default

- Status: Accepted
- Date: 2026-11-21

## Context

Privacy is a product requirement.

## Decision

No analytics. Crash reports are opt-in and shown to the user before sending. Logs never contain message content or tokens.

## Consequences

Performance data comes from `cargo xtask perf` and volunteers, not from users.
