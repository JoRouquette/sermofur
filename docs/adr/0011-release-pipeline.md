English | [Français](../fr/adr/0011-release-pipeline.md)

# ADR 0011-release-pipeline — Semantic versioning and release pipeline

Date: 2026-10-07. Status: accepted.

## Context
Sermofur is published as a .NET tool. Versions were set by hand in the project file, nothing
verified the code outside Windows, and publishing meant local commands with a long-lived
nuget.org key. The commits already follow Conventional Commits.

## Decision
- Versions follow Semantic Versioning and are computed by semantic-release from the commits on
  `main` (`fix`/`perf`/`revert` patch, `feat` minor, breaking change major; other types do not
  release). Tags are `vX.Y.Z`.
- One workflow, `ci.yml`, builds and tests every pull request and push on Windows, Linux and
  macOS. On `main`, a dry run decides whether a release is due; the `release` job then waits for
  the maintainer's approval in the `release` environment, tags and creates the GitHub Release.
- The `package` job builds the tag (it must be on `main`) without any npm code, attests the
  package provenance and attaches it to the release. The `publish` job pushes it to nuget.org.
- Publication uses nuget.org Trusted Publishing (OIDC): no long-lived key is stored. Only the
  `publish` job, in the `nuget` environment that the nuget.org policy names, can obtain a
  nuget.org key; it runs no repository code, no npm and no build. The semantic-release jobs only
  hold a GitHub token.
- The pipeline never commits to `main`: the version is passed to the build and the release notes
  live in the GitHub Releases. The repository keeps the development version `0.0.0-dev`, and the
  tool reads its version from its assembly.
- A manual run republishes an existing tag (first release, failed publication); it goes through
  the same approval. It reuses the package attached to the release only if this workflow
  attested it, and builds the tag otherwise, so the release asset and nuget.org always hold the
  same bytes.

## Alternatives
release-please: a release pull request gives a review point, but adds a bot pull request to each
change; the approval of the `release` environment gives the same control. Committing the version
and a changelog back to `main` (semantic-release git plugin): needs a token that bypasses the
branch protection. A manual tag with MinVer: no computed version, no release notes. Building the
package inside semantic-release (exec plugin): the npm tooling would produce the published bytes.

## Consequences
Contributors must use Conventional Commits, and pull requests are not squashed. Node is needed in
CI only. A breaking change in 0.x releases 1.0.0. Each release needs an approval; a rejected
release is included in the next one. The workflow file name and the `nuget` environment are part
of the nuget.org policy.

Accepted risk: the `plan` dry run needs `contents: write` (semantic-release checks that it could
push the tag) and runs the npm tooling on every push to `main` without approval. A compromised
dependency could then create a tag on a commit of `main`, a GitHub Release or an asset, but it
cannot change the published package: `package` reuses an asset only if this workflow attested
it on `main` for that version, and otherwise stops, or builds the tag, which must be on `main`,
with the packaging script of `main`; `publish` pushes only that package.
Dependencies are pinned, locked and installed without scripts.
