# Repository-owned registry release

Release versions are read from package manifests, not hardcoded in workflows. Merge the version/changelog PR and wait for all required default-branch workflows in `.github/registry-release.json`. Push the version tag to start archive preparation. Private packages and `publish=false` crates remain outside this flow.

A single-package repository accepts `v<version>`. Monorepos use `npm/<scope>--<name>/v<version>` or `cargo/<crate-name>/v<version>`; one tag selects exactly one package. The tag must point to a commit already accepted on the default branch. Configure a GitHub tag ruleset and required reviewers for `npm-public` or `crates-io-public`. Do not move a published tag.

The prepare job has no publication credential. It builds, inspects and binds an archive to source/tag/package/version/SHA256. The publication job downloads that exact artifact ID and checks its bytes. `REGISTRY_PUBLISH_ENABLED=true` enables publication only after the registry's Trusted Publisher is registered for this repository, the matching `publish-npm.yml` or `publish-cargo.yml`, and environment. No long-lived publication token is required. Keep this variable disabled until registration is verified.

For npm, `NPM_PUBLICATION_MODE=stage` (default) uses `npm stage publish` and still requires npm maintainer approval. For automatic formal publication, register the Trusted Publisher with allowed action `npm publish` and set `NPM_PUBLICATION_MODE=direct`. Token publication remains prohibited. Direct publication verifies official npm integrity; crates.io publication verifies its official SHA256. Initial package/trust registration and environment protections are configuration steps, not inferred from source CI.

Source PR CI verifies release-selection and archive-refusal tests without publishing. Tag preparation is independently gated by existing functional CI. No release tag, registry setting, production deployment or publication is created by adding this source configuration.
