# 0014. Apache-2.0

- Status: Accepted
- Date: 2026-11-21

## Context

The maintainer chose a permissive license with an explicit patent grant (the independent review recommended the same).

## Decision

The whole repository is Apache-2.0 with a NOTICE file. Contributions are accepted under the DCO (`git commit -s`). `cargo deny` allows only compatible licenses; bundled fonts (OFL-1.1) keep their license files.

## Consequences

Easy adoption, including closed forks.
