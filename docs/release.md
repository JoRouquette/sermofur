English | [Français](fr/release.md)

# Release process

Sermofur follows [Semantic Versioning](https://semver.org/). Versions are computed from the
[Conventional Commits](https://www.conventionalcommits.org/) that reach `main`, by
[semantic-release](https://semantic-release.gitbook.io/), in the workflow
`.github/workflows/ci.yml` ([ADR 0011](adr/0011-release-pipeline.md)).

## What happens on each change

| Event | Jobs |
|---|---|
| Pull request to `main` | `build` on Windows, Linux and macOS: locked restore, formatting check, Release build, tests, package (Linux); `release-notes`: renders sample release notes with the locked semantic-release tooling |
| Push to `main` | `build` and `release-notes`, then `plan`: a semantic-release dry run that tells whether a release is due |
| Release due | `release` waits for the approval of the `release` environment, then creates the tag `vX.Y.Z` and the GitHub Release; `package` builds the tag, attests the package and attaches it to the release; `publish` pushes it to nuget.org |
| Manual run with a tag | `republish` waits for the same approval; `package` reuses the package attached to that release if this workflow attested it on `main`, or builds the tag; `publish` pushes it to nuget.org |

| Commit on `main` | Version |
|---|---|
| `fix:`, `perf:`, `revert:` | patch |
| `feat:` | minor |
| `!` after the type, or a `BREAKING CHANGE:` footer | major (also from 0.x: semantic-release then goes to 1.0.0) |
| `docs:`, `test:`, `refactor:`, `build:`, `ci:`, `style:`, `chore:` | no release |

No commit is ever written to `main` by the pipeline: the version is passed to the build
(`-p:Version=X.Y.Z`) and the release notes live in the GitHub Releases. In the repository, the
version stays `0.0.0-dev` (`Directory.Build.props`); `smf --help` shows the version of the build
and links to the CLI reference of its tag, or of `main` for a development build.

The package is built from the tag by `package`, which runs no npm code, and only `publish` can
obtain a nuget.org key: it runs no repository code, no npm and no build. The jobs that run
semantic-release and its npm dependencies only hold a GitHub token.

Dependabot proposes weekly updates of the actions, NuGet and npm packages. Its NuGet updates use
the `build:` type, which does not release. When an update fixes a vulnerability in a runtime
dependency, add a `fix(deps): ...` commit naming the advisory to the Dependabot branch before
merging it, so that a patch release ships the fix.

## One-time setup (maintainer)

These settings live outside the repository and are done once.

1. **Protect `main`** (Settings → Rules → Rulesets → New branch ruleset, target `main`): require a
   pull request (0 approvals for a single maintainer, who cannot approve their own pull request);
   require the status checks `build (windows-latest)`, `build (ubuntu-latest)`,
   `build (macos-latest)` and `release-notes`; block force pushes and deletions. In Settings →
   General, allow merge commits and rebase merging, disable squash merging.
2. **Protect release tags** (New tag ruleset, target `v*`): block updates and deletions, so that
   a published tag can never be moved or removed.
3. **Environment `release`** (Settings → Environments → New environment): required reviewer = the
   maintainer; deployment branches and tags = `main` only. It gates tagging and publication.
4. **Environment `nuget`**: no reviewer; deployment branches and tags = `main` only. It is the
   only environment that nuget.org trusts.
5. **nuget.org Trusted Publishing** (nuget.org → your user name → Trusted Publishing → add a
   policy): owner `JoRouquette`, repository `sermofur`, workflow file `ci.yml`, environment
   `nuget`; scope *Push new packages and package versions* (the first push creates the package),
   *Unlist or relist* unchecked; glob patterns and packages: `Sermofur` only.
6. **Secret `NUGET_USER`** (Settings → Secrets and variables → Actions, or as an environment
   secret of `nuget`): the nuget.org profile name (not the email address). It is not a
   credential: nuget.org issues a one-hour key to the workflow in exchange for its OIDC token.

## First release (0.1.0)

semantic-release starts at 1.0.0 when no tag exists. The first version is therefore tagged by
hand on the commit already published on `main`, **before** the pull request that adds the
pipeline is merged:

```powershell
git tag -a v0.1.0 -m "Sermofur 0.1.0" <commit>
git push origin v0.1.0
```

Then, in this order:

1. Merge the pull request that adds the pipeline. The push to `main` runs `plan`, which may
   propose the next version (0.1.1 for a `fix:`); **reject** that `release` job.
2. Actions → *CI and release* → *Run workflow*, branch `main`, tag `v0.1.0`; approve the
   `republish` job and wait until `publish` has pushed 0.1.0.
3. Open the run rejected at step 1 and use *Re-run all jobs*: `plan` computes the next version
   from `v0.1.0` again, and `release` waits for approval. If `main` has moved since, skip this:
   the next push carries the release.

The next releases start from this tag. If a `release` job ever proposes 1.0.0 by mistake,
reject it. The notes of this first release are generated by GitHub, not by semantic-release.

## Approving a release

When a release is due, the run shows the `release` job waiting for review. Check the version
computed by `plan` (in its log) and the commits since the last tag, then approve. Reject to skip
the release: the commits stay on `main` and the next release includes them.

If `main` moved while the approval was pending, semantic-release releases nothing and the job
fails on purpose ("nothing was released"): the run started by the newer push carries the release.

## Recovering

- **`publish` failed** (nuget.org unavailable, policy missing): use *Re-run failed jobs* on that
  run while its `release-package` artifact exists; the same package is pushed.
- **The artifact expired, or `package` failed**: run `republish` with that tag. A package
  already attached to the release is reused after its attestation is verified, so the release
  asset and nuget.org keep the same bytes; a package that this workflow did not attest stops the
  job. Without a package, the tag is built, attested and attached. The push uses
  `--skip-duplicate`, so a version already on nuget.org is not an error. Do not re-run the
  `release` job: the tag exists, so it releases nothing.
- **Never** delete a published tag or reuse a version number: publish a new patch instead.
- A release run stopped before the tag: nothing was published; the next push to `main` computes
  the release again.
