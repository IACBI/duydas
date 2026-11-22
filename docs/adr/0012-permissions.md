# 0012. Deny-by-default u64 permission flags

- Status: Accepted
- Date: 2026-11-21

## Context

Authorization bugs (IDOR, stale permissions) are the most likely serious server bugs.

## Decision

`duydas-core::permissions` resolves a hand-written u64 flag set (ADMINISTRATOR implies all; unknown bits from newer servers are dropped with `from_bits_truncate`). Every handler goes through one `authorize(resource, action)`; permission-sensitive writes re-check in the transaction; revocation closes live subscriptions and calls.

## Consequences

Property tests and IDOR tests target a single code path.
