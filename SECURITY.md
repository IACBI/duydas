# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Report them privately through GitHub: **Security → Report a vulnerability** on this repository
([direct link](https://github.com/IACBI/duydas/security/advisories/new)). Include what you found, how to reproduce it, and what an attacker could do with it.

What to expect:

- Acknowledgement within 7 days.
- An initial assessment and planned fix timeline within 14 days.
- Coordinated disclosure: we aim to publish a fix and advisory within 90 days of the report, sooner for actively exploited issues. We will credit you unless you prefer otherwise.

## Supported versions

Duydaş is pre-release (0.x). Only the latest commit on `main` receives security fixes until the first stable release; this table will list supported release lines after that.

## Scope

In scope: the desktop client, the server, the update mechanism, build and release workflows, and the cryptographic design in `docs/threat-model.md`.

Out of scope: vulnerabilities in self-hosted deployments caused by configuration that the documentation advises against, and denial of service through volumetric traffic.

## Handling secrets in this repository

No credentials, tokens or signing keys belong in this repository. Secret scanning with push protection is enabled. Release signing keys live only in protected GitHub Environments that require maintainer approval.
