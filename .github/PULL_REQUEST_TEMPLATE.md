## What and why

<!-- What does this change, and what problem does it solve? Link issues with "Closes #123". -->

## How it was tested

<!-- Commands run, platforms tried, screenshots for UI changes (ideally in more than one language). -->

## Checklist

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `typos` pass
- [ ] `cargo xtask i18n-check` passes; new UI text is in `locales/en-US` with a context comment (no hardcoded strings)
- [ ] New and changed UI text has Turkish and Japanese translations
- [ ] Docs/ADR updated if behavior or architecture changed
- [ ] Commits are signed off (`git commit -s`, see CONTRIBUTING.md)
