# Threat model (draft)

Status: **draft for Phase 0.** Each phase that touches security updates this document before it ships.

## Assets

Account credentials and sessions; message content; who-talks-to-whom metadata; voice audio; attachments; the update channel (it runs code on users' machines); release signing keys.

## Adversaries considered

1. Network attackers (passive and active) between clients and servers.
2. A malicious or compromised community member or moderator.
3. A stolen or shared device.
4. A compromised server or hosting provider (for E2EE content only).
5. A compromised dependency or CI pipeline.

## Protection matrix (target state)

| Data | In transit | At rest on server | Server can read? |
|---|---|---|---|
| Community channel messages | TLS 1.3 (HTTPS/WSS) | Database encryption is the operator's job | **Yes**: moderation, search and large groups need it |
| Direct and group DMs (Phase 5) | TLS 1.3 + MLS | Ciphertext only | No, from the conversation's start boundary |
| Voice in community channels | DTLS-SRTP (hop-by-hop to the SFU) | Not stored | SFU can |
| Voice in DM calls (Phase 5) | DTLS-SRTP + SFrame | Not stored | No |
| Attachments | TLS 1.3 | Stored as uploaded | Yes (community), No (E2EE DMs) |
| Metadata (who, when, which channel) | TLS 1.3 | Stored | Yes. Minimised, but not hidden |
| Session tokens | TLS 1.3 | SHA-256 hashes only | No |
| Local cache | — | OS-protected profile directory; encryption decided before E2EE ships | — |

## Key decisions so far

- Short-lived access tokens (15 min) and rotating refresh tokens with reuse detection; revocation closes live connections within 5 s.
- Passwords hashed with Argon2id; concurrent verifications are capped so a login flood cannot exhaust server memory.
- No arbitrary URL fetching on the server until SSRF controls (egress allowlist, redirect limits, decode budgets) exist.
- Updates are accepted only with signed metadata binding version, channel, platform, architecture, digest and size, with expiry and downgrade protection (see ADR 0010).
- No telemetry. Crash reports are opt-in and shown to the user before sending.

## Open questions

- Device verification UX and recovery for MLS (Phase 5).
- Whether decrypted E2EE messages may persist in the local cache, and how that interacts with backups.
- Metadata minimisation for presence and typing indicators.
