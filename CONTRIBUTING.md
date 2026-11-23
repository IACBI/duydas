# Contributing to Duydaş

Thanks for helping. This guide covers setup, the checks your change must pass, and how we work.

## Setup

1. Install Rust with [rustup](https://rustup.rs). `rust-toolchain.toml` selects the stable channel; the minimum supported version is 1.98.
2. Linux only: install the development packages for your desktop (Wayland/X11, xkbcommon) if the build asks for them.
3. Build and run:

   ```sh
   cargo run
   ```

## Before you open a pull request

Run the checks CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked    # or: cargo test --workspace --locked
cargo test --workspace --doc --locked
cargo xtask i18n-check
typos
```

Install [cargo-nextest](https://nexte.st) and [typos](https://github.com/crate-ci/typos) (`cargo install cargo-nextest typos-cli`) to run them locally. CI also runs `cargo deny --locked check` ([cargo-deny](https://github.com/EmbarkStudios/cargo-deny)) for licenses, advisories, bans and sources, builds rustdoc with warnings denied, and checks Rust 1.98.

If you change the Unicode version or the table generator, run `cargo xtask unicode-gen` and commit the tables and conformance data it writes. The first run downloads the Unicode Character Database with `curl`.

## Ground rules

- **No hardcoded UI text.** Every string the app shows goes into `locales/en-US/*.ftl` with a comment explaining its context, and is used through `Key::…`. See [docs/i18n-guide.md](docs/i18n-guide.md).
- **No new dependencies.** Duydaş writes its stack in-house ([ADR 0021](docs/adr/0021-in-house-implementation.md)). The only libraries allowed are the Opus codec and audited cryptography (TLS, DTLS-SRTP and the primitives they need). Each needs an entry in `deny.toml` and a line in ADR 0021 before it is added. The remaining third-party crates (Iced and its dependencies) are being replaced step by step. Development tools such as cargo-deny and typos never ship and are not dependencies.
- **`unsafe` only in `duydas-sys`.** OS access goes through it. Every block carries a `SAFETY:` comment, does one unsafe operation, and sits behind a safe function. Every other crate inherits `unsafe_code = "forbid"`.
- **Comments explain why, not what.** Public items get doc comments.
- **Architecture changes need an ADR** in `docs/adr/` (copy the template).
- Never commit credentials, tokens, keys or `.env` files.

## Commits

- Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:`).
- Sign off every commit to certify the [Developer Certificate of Origin](https://developercertificate.org/): `git commit -s`. By signing off you state that you have the right to submit the work under the project's license (Apache-2.0).
- Keep pull requests focused. One logical change per PR is easiest to review.

## Labels and milestones

Every issue and pull request ends up with one `type:` label and at least one `area:` label. The issue forms and Dependabot set some of them; maintainers add the rest during triage and set the milestone.

| Group | Labels |
|---|---|
| Type | `type: bug`, `type: feature`, `type: docs`, `type: performance`, `type: security`, `type: refactor`, `type: ci`, `type: chore` |
| Area | `area: i18n`, `area: ui`, `area: platform`, `area: sys`, `area: core`, `area: client`, `area: server`, `area: voice`, `area: build` |
| Status | `status: triage` (set by the issue forms), `status: blocked` |
| Other | `priority: high`, `priority: low`, `breaking-change`, `new-language`, `accessibility`, `tracking`, `dependencies` (set by Dependabot), `good first issue`, `help wanted` |

Milestones follow the roadmap in [`docs/architecture.md`](docs/architecture.md) and the in-house replacement steps in [ADR 0021](docs/adr/0021-in-house-implementation.md); larger efforts have a `tracking` issue.

Report security vulnerabilities privately as described in [SECURITY.md](SECURITY.md), never in a public issue.

## Translations

Translators don't need Rust. Edit the `.ftl` files under `locales/<language>/` and run `cargo xtask i18n-check`, or open a [translation issue](https://github.com/IACBI/duydas/issues/new?template=translation.yml). To add a language, open a [new language issue](https://github.com/IACBI/duydas/issues/new?template=new_language.yml) first.

## Reporting bugs and security issues

Use the issue forms for bugs and features. Report security issues privately; see [SECURITY.md](SECURITY.md).
