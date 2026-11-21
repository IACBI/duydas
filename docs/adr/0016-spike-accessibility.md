# 0016. Spike: screen-reader support

- Status: Pending
- Date: 2026-11-21

## Context

Iced 0.14 exposes no accessibility tree. AccessKit integration exists only as a draft PR (#3111).

## Decision

Build the shell against the draft branch and test read, navigate, compose and send with Narrator/NVDA, VoiceOver and Orca. Decide by the end of Phase 0: upstream contribution, maintained patch, or toolkit re-evaluation.

## Consequences

Screen-reader support is a 1.0 gate.
