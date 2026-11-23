# Governance

## Today: maintainer-led

Duydaş is maintained by 𝓐.𝓒.𝓑 (@IACBI), who has final say on direction, merges and releases. Decisions with lasting impact go through an Architecture Decision Record in `docs/adr/` and a pull request, so the reasoning stays public.

## Becoming a maintainer

Contributors who have landed sustained, high-quality work and reviews over several months may be invited as maintainers. Maintainers can merge pull requests in their area and take part in release decisions. Code owners are listed in [`.github/CODEOWNERS`](.github/CODEOWNERS).

## Releases and signing keys

There are no releases yet. When there are:

- A maintainer cuts releases from `main`, following [`docs/release.md`](docs/release.md).
- Signing credentials used by CI live only in protected GitHub Environments. The update metadata root key is not one of them: the lead maintainer holds it offline with a sealed backup. Rotation and compromise recovery are documented in `docs/release.md` before the first signed release.
- At least two people will hold release capability before 1.0, so the project does not depend on one person's availability.

## Succession

If the lead maintainer becomes unavailable for more than 90 days without notice, the remaining maintainers may appoint a new lead by consensus and request repository ownership transfer through GitHub.

## Code of Conduct

Everyone participating follows the [Code of Conduct](CODE_OF_CONDUCT.md).
