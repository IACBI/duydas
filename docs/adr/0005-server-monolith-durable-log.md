# 0005. Single modular server with a transactional event log

- Status: Accepted
- Date: 2026-11-21

## Context

Self-hosting should need one binary and PostgreSQL. In-process broadcast alone can drop events for slow receivers or after a crash between commit and broadcast.

## Decision

One `duydas-server` binary (axum, tokio, sqlx) with auth, gateway, rest, sfu and media modules. Each message and its event-log row commit in one transaction with a server-assigned per-channel sequence number. Broadcast only wakes connections; per-connection queues are bounded and overflow triggers an explicit resync. Clients deduplicate with client-generated send ids. Multi-node fan-out (NATS) only after a load test shows the single-node limit.

## Consequences

Lossless reconnect is designed in from Phase 1.
