# Governance

## Today: maintainer-led

Duydaş is maintained by 𝓐.𝓒.𝓑 (@IACBI), who has final say on direction, merges and releases. Decisions with lasting impact go through an Architecture Decision Record in `docs/adr/` and a pull request, so the reasoning stays public.

## Becoming a maintainer

Contributors who have landed sustained, high-quality work and reviews over several months may be invited as maintainers. Maintainers can merge pull requests in their area and take part in release decisions. The list lives in `.github/CODEOWNERS`.

## Releases and signing keys

- Releases are cut from `main` by a maintainer, following `docs/release.md`.
- Code-signing and update-signing credentials are held only in protected GitHub Environments. The offline update root key is stored by the lead maintainer with a sealed backup; its rotation procedure is documented in `docs/release.md` before the first signed release.
- At least two people will hold release capability before 1.0, so the project does not depend on one person's availability.

## Succession

If the lead maintainer becomes unavailable for more than 90 days without notice, the remaining maintainers may appoint a new lead by consensus and request repository ownership transfer through GitHub.

## Code of Conduct

Everyone participating follows the [Code of Conduct](CODE_OF_CONDUCT.md).
