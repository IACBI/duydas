# Contributing to Duydaş

Thanks for helping. This guide covers setup, the checks your change must pass, and how we work.

## Setup

1. Install Rust with [rustup](https://rustup.rs). The repository pins the `stable` channel in `rust-toolchain.toml`.
2. Linux only: install the development packages for your desktop (Wayland/X11, xkbcommon) if the build asks for them.
3. Build and run:

   ```sh
   cargo run
   ```

## Before you open a pull request

Run the same checks CI runs:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask i18n-check
```

`cargo deny check` (from [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)) checks licenses and advisories when you add dependencies.

## Ground rules

- **No hardcoded UI text.** Every string a user can see goes into `locales/en-US/*.ftl` with a comment explaining its context, and is used through `Key::…`. See [docs/i18n-guide.md](docs/i18n-guide.md).
- **Keep dependencies few.** A new crate needs a reason in the PR description: what it replaces, its maintenance status, and its license.
- **No `unsafe`** outside modules that the architecture explicitly allows (none yet).
- **Comments explain why, not what.** Public items get doc comments.
- **Architecture changes need an ADR** in `docs/adr/` (copy the template).
- Never commit credentials, tokens, keys or `.env` files.

## Commits

- Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:`).
- Sign off every commit to certify the [Developer Certificate of Origin](https://developercertificate.org/): `git commit -s`. By signing off you state that you have the right to submit the work under the project's license (Apache-2.0).
- Keep pull requests focused. One logical change per PR is easiest to review.

## Labels and milestones

Every issue and pull request carries one `type:` label and at least one `area:` label. Maintainers set the milestone and add the rest.

| Group | Labels |
|---|---|
| Type | `type: bug`, `type: feature`, `type: docs`, `type: performance`, `type: security`, `type: refactor`, `type: ci`, `type: chore` |
| Area | `area: i18n`, `area: ui`, `area: platform`, `area: sys`, `area: core`, `area: client`, `area: server`, `area: voice`, `area: build` |
| Status | `status: triage` (set by the issue forms), `status: blocked` |
| Other | `priority: high`, `priority: low`, `breaking-change`, `new-language`, `accessibility`, `tracking`, `good first issue`, `help wanted` |

Milestones follow the roadmap in [`docs/architecture.md`](docs/architecture.md) and the in-house replacement steps in [ADR 0021](docs/adr/0021-in-house-implementation.md); larger efforts have a `tracking` issue.

Report security vulnerabilities privately as described in [SECURITY.md](SECURITY.md), never in a public issue.

## Translations

Translators don't need Rust. Edit the `.ftl` files under `locales/<language>/` and run `cargo xtask i18n-check`, or open a [translation issue](https://github.com/IACBI/duydas/issues/new?template=translation.yml). To add a language, open a [new language issue](https://github.com/IACBI/duydas/issues/new?template=new_language.yml) first.

## Reporting bugs and security issues

Use the issue forms for bugs and features. Report security issues privately; see [SECURITY.md](SECURITY.md).
