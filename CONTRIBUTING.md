English | [Français](CONTRIBUTING.fr.md)

# Contributing to Sermofur

Thank you for your interest. Issues and pull requests are welcome.

## Developer Certificate of Origin

Contributions are accepted under the [Developer Certificate of Origin 1.1](https://developercertificate.org/)
(DCO). By signing off a commit, you certify that you wrote the change or otherwise have the right
to submit it under the project's license, the [Apache License, Version 2.0](LICENSE).

Every commit must carry a `Signed-off-by` line with your real name and email:

```text
Signed-off-by: Jane Doe <jane.doe@example.com>
```

`git commit -s` adds it for you. To sign off commits you already made on your branch, run
`git rebase --signoff <base>` and force-push the branch. **A pull request containing a commit
without sign-off is not merged.** A GPG or SSH signature is neither required nor sufficient: only
the `Signed-off-by` line counts.

## Prerequisites

- [.NET 10 SDK](https://dotnet.microsoft.com/download) (see `global.json`).
- [CSharpier](https://csharpier.com/) for formatting.

## Build, test, format

```powershell
dotnet restore --locked-mode
dotnet build
dotnet test
csharpier format .
```

Run `csharpier check .` before opening a pull request. Process tests launch the built CLI: do not
use `dotnet test --no-build` after changing code. More: [developer setup](docs/developer-setup.md)
and [verification](docs/verification.md).

## Code style

The analyzers enforce the main rules at build time, with warnings treated as errors:

- explicit types instead of `var` (`var` only for anonymous types);
- braces on every block, even single-line ones;
- nullable reference types enabled, no warning silenced with `!` without reason.

Keep the layers apart: Domain has no external dependency, Application carries the rules,
Infrastructure implements the ports, the CLI composes. Code, identifiers, comments, CLI messages
and commit messages are in English. Every behavior change comes with its tests; a bug fix starts
with the test that reproduces it.

Do not change error codes, exit codes, JSON field names or the persisted format without a
discussion in an issue first: they are contracts.

## Commits and pull requests

Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`,
`test:`, `refactor:`, `chore:`… One topic per pull request, with a description of the change and
of how it was verified.

## Documentation

English is the reference language; the French translation lives in `README.fr.md` and
`docs/fr/`. If you can, update both; otherwise say so in the pull request and the French page
will be updated afterwards.

## Specifications

The maintainer keeps the product specifications in a Spec Kit workshop outside this repository.
Contributors are not required to use it: an issue and a clear pull request are enough. Durable
decisions are recorded as [ADRs](docs/adr/).
