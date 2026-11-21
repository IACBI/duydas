# 0001. Use Iced 0.14 from crates.io

- Status: Accepted
- Date: 2026-11-21

## Context

We need a native Rust GUI toolkit with low idle cost, good text shaping and cross-platform support. Iced 0.14 (Dec 2025) brings reactive rendering, IME support, an animation API and headless testing. It lacks screen-reader support (AccessKit PR #3111 is a draft) and has an open Windows Japanese IME bug (#3189). Master (0.15-dev) is mid-refactor.

## Decision

Depend on released Iced 0.14.x. A git-rev pin is allowed only for a blocker fix (IME, accessibility) that a spike has verified. Custom widgets and theme code live in `duydas-ui`, so upgrades touch one crate. The go/no-go for Iced is decided at the end of Phase 0 from ADRs 0015 and 0016.

## Consequences

Reproducible builds and a yearly upgrade window. Accessibility depends on upstream progress, so it is a 1.0 gate with a fallback evaluation.
