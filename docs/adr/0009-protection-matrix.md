# 0009. Protection matrix and E2EE scope

- Status: Accepted
- Date: 2026-11-21

## Context

Promising end-to-end encryption everywhere would block moderation, search and large communities.

## Decision

Community channels are server-readable over TLS 1.3. DMs and group DMs move to MLS (openmls) in Phase 5 with an explicit history boundary and no silent downgrade; DM calls get SFrame. The full matrix lives in `docs/threat-model.md`.

## Consequences

Honest guarantees users can understand.
