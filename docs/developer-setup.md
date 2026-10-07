English | [Français](fr/developer-setup.md)

# Developer setup

.NET 10 SDK (`global.json` pins 10.0.201 with `rollForward: latestFeature`) and Git.
[CSharpier](https://csharpier.com/) is a local tool pinned in `.config/dotnet-tools.json`
(`dotnet tool restore`). `NuGet.Config` restricts package sources to nuget.org.
Dependencies are pinned and restored from lock files (`packages.lock.json`).
.NET CLI telemetry: set `DOTNET_CLI_TELEMETRY_OPTOUT=1` for the development tools if you wish.
The Sermofur runtime itself collects no telemetry.

```powershell
dotnet tool restore
dotnet restore --locked-mode
dotnet build --no-restore
dotnet test --no-restore
dotnet csharpier format .
dotnet csharpier check .
```
Run `dotnet csharpier check .` before review and commit. Nullable reference types are enabled
and warnings are errors; the code style rules (explicit types, braces) are enforced at build time.
Tests create separate temporary folders and delete them; they read no user data.
Process tests launch the built CLI: do not run `dotnet test --no-build` after a change.

The first transactional schema 0→1 is created for a new database; reading refuses any other
version. No migration of an existing instance is implemented: migration, backup and tests come
before any schema v2.
The CI builds and tests every pull request on Windows, Linux and macOS
([release process](release.md)).
See [verification](verification.md) for what is checked and on which platform.
