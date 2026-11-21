# 0013. pulldown-cmark with a strict rendered subset

- Status: Accepted
- Date: 2026-11-21

## Context

A custom markdown parser is unnecessary risk; Iced already depends on pulldown-cmark.

## Decision

Parse with pulldown-cmark and render only an allowed subset (emphasis, code, quotes, lists, links). No HTML. Links show their real target; non-allowlisted domains need confirmation. The parse→render boundary is fuzzed.

## Consequences

Less code to maintain, and a well-known parser.
