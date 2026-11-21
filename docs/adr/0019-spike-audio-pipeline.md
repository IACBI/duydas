# 0019. Spike: audio pipeline

- Status: Pending
- Date: 2026-11-21

## Context

Echo cancellation options need native builds (webrtc-audio-processing) or OS voice processing; latency must be measured physically.

## Decision

Build cpal → noise suppression/AEC → Opus → str0m loopback on all three OSes; measure physical mouth-to-ear latency, CPU per stream, and build complexity. Pass = latency and CPU within budget and a reproducible build on CI.

## Consequences

Decides the AEC approach for Phase 4.
