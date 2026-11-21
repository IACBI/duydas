# 0006. Voice SFU built on str0m

- Status: Accepted
- Date: 2026-11-21

## Context

Voice needs an SFU, NAT traversal and a client stack. str0m is a sans-IO Rust WebRTC library usable on both ends.

## Decision

Build the SFU and client transport on str0m. Opus 48 kHz / 20 ms with FEC and DTX; cpal for audio I/O; nnnoiseless for noise suppression; echo cancellation chosen by spike (ADR 0019). ICE-TCP/TURN reachability is part of the Phase 4 gate. Voice tokens are short-lived and bound to user and room. If str0m misses the Phase 4 go/no-go criteria, self-hosted LiveKit is the fallback.

## Consequences

Single-language stack and deterministic impairment tests; the project owns SFU operations.
