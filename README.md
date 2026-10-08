English | [Français](https://github.com/JoRouquette/sermofur/blob/main/README.fr.md)

# Sermofur

[![CI and release](https://github.com/JoRouquette/sermofur/actions/workflows/ci.yml/badge.svg)](https://github.com/JoRouquette/sermofur/actions/workflows/ci.yml)
[![NuGet](https://img.shields.io/nuget/v/Sermofur)](https://www.nuget.org/packages/Sermofur)

Sermofur is a local cognitive runtime: a memory that an assistant, or you, can consult and
**contest**. It does not store "facts"; it stores **claims** backed by **evidence**, and lessons
learned (**RETEX**, from the French *retour d'expérience*), each in an isolated **scope**, with
their **provenance** and an **explained confidence**. The goal is to make future decisions better
through experience without turning past mistakes into permanent truths.

Sermofur runs next to the language model, never inside it: the weights of the host LLM are never
modified. Sermofur is independent of any LLM provider.

The name comes from the author's tabletop world, where Sermofur is an engraved script that keeps
what people forget: *"When the eye forgets, the hand remembers."*

Technical identity: repository `sermofur`, .NET projects `Sermofur.*`, NuGet tool
package `Sermofur`, command `smf`, instance marker `.sermofur`.

## Cognitive model

USER ≠ TRUTH, LLM ≠ TRUTH, MEMORY ≠ TRUTH.

- A **claim** is a statement in a scope. Its confidence is computed when it is read, from its
  evidence, and every level comes with its reasons.
- **Evidence** is declared support for a claim (source code, documentation, observation…).
  Evidence sharing the same lineage counts as one origin: repetition does not raise confidence.
  Evidence declared by an LLM never reinforces a claim, and declared `execution` evidence alone
  cannot verify a fact.
- A **RETEX** stays a draft until a learning decision is made.
- **Scopes** (workspace → client → project → repository → task) isolate memories: a context sees
  its own scope and its ancestors, never its siblings or descendants.
- Every change is kept in an auditable **history**.

## Status: 0.1 vertical slice

Delivered: the `smf` CLI, local instances, scopes, Claim/Evidence/RETEX storage in SQLite,
history, Markdown projections and diagnostics (`doctor`).

**Not delivered yet**: background daemon, MCP bridge, Laya integration (System 1 model),
sources/full-text search/recall/challenge, learning and consolidation, desktop Inspector UI.
These appear in the docs as designs only; no command pretends to provide them.

No telemetry, synchronization, network connection or source ingestion at run time.

## Install

With the [.NET 10 SDK](https://dotnet.microsoft.com/download), from nuget.org (running the tool
then only needs the .NET 10 runtime):

```powershell
dotnet tool install --global Sermofur
smf --help
```

Releases and their notes: [GitHub Releases](https://github.com/JoRouquette/sermofur/releases).

## Install from source

Requirements: the [.NET 10 SDK](https://dotnet.microsoft.com/download). A build from source carries
the development version `0.0.0-dev`.

```powershell
git clone https://github.com/JoRouquette/sermofur.git
cd sermofur
dotnet restore --locked-mode
dotnet build --no-restore
dotnet pack src/Sermofur.Cli -c Release -o artifacts/packages --no-restore
dotnet tool install Sermofur --version 0.0.0-dev --tool-path artifacts/tools --configfile nuget.local.config --no-cache
./artifacts/tools/smf --help
```

`nuget.local.config` lists only the local package folder, so the install never takes a package of
the same name from nuget.org. Use `--global` instead of `--tool-path artifacts/tools` to put `smf`
on your `PATH`. The tool requires the .NET 10 runtime; a self-contained distribution is planned.
Without installing, `dotnet run --project src/Sermofur.Cli -- --help` runs the CLI from the
sources.

## Quickstart

```powershell
cd ~/work                        # this folder becomes the instance root
smf init                         # creates ~/work/.sermofur
mkdir acme
smf scope add acme client workspace acme
cd acme                          # the context is now the "acme" scope
smf claim add "The billing API paginates with cursors" --origin user --json
smf evidence add <CLAIM_ID> source_code "src/Billing/Pagination.cs" --lineage billing-repo --origin user
smf claim show <CLAIM_ID>        # claim, evidence, explained confidence, history
smf doctor                       # read-only health check
smf export                       # rebuilds the Markdown projections of visible objects
```

`<CLAIM_ID>` is the `id` returned by `claim add`. The scope always comes from the working
directory (or `--path`), never from an argument: from another client's folder, the `acme` claim
is invisible. `--origin user|llm` is mandatory on every `add` that records memory. Add `--json`
to any command for machine-readable output. Full reference: [CLI](https://github.com/JoRouquette/sermofur/blob/main/docs/cli.md).

## Documentation

- [Architecture](https://github.com/JoRouquette/sermofur/blob/main/docs/architecture.md), [developer setup](https://github.com/JoRouquette/sermofur/blob/main/docs/developer-setup.md),
  [verification](https://github.com/JoRouquette/sermofur/blob/main/docs/verification.md).
- [CLI](https://github.com/JoRouquette/sermofur/blob/main/docs/cli.md), [instance format](https://github.com/JoRouquette/sermofur/blob/main/docs/instance-format.md),
  [scope model](https://github.com/JoRouquette/sermofur/blob/main/docs/scope-model.md).
- [Memory model](https://github.com/JoRouquette/sermofur/blob/main/docs/memory-model.md), [security model](https://github.com/JoRouquette/sermofur/blob/main/docs/security-model.md).
- [MCP](https://github.com/JoRouquette/sermofur/blob/main/docs/mcp-integration.md) and [Laya](https://github.com/JoRouquette/sermofur/blob/main/docs/laya-integration.md): future contracts, not
  implementations.
- [Release process](https://github.com/JoRouquette/sermofur/blob/main/docs/release.md): versions, CI, publication.
- Architecture decision records: [docs/adr](https://github.com/JoRouquette/sermofur/tree/main/docs/adr).

Specifications are written in a Spec Kit workshop kept outside this repository; durable
decisions are extracted as ADRs.

## Output encoding

stdout and stderr are UTF-8 without BOM. `smf` switches the console code page only when stdout
or stderr reaches the console, and restores it on normal exit, on handled errors and on Ctrl+C;
restoration is not guaranteed if the process is killed. In Windows PowerShell, a caller that
captures the output first runs
`[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)`. Details: [CLI](https://github.com/JoRouquette/sermofur/blob/main/docs/cli.md).

## Contributing

Contributions are welcome under the Developer Certificate of Origin: every commit is signed off
(`git commit -s`). See [CONTRIBUTING.md](https://github.com/JoRouquette/sermofur/blob/main/CONTRIBUTING.md).

## License

Licensed under the [Apache License, Version 2.0](https://github.com/JoRouquette/sermofur/blob/main/LICENSE); attribution in
[NOTICE](https://github.com/JoRouquette/sermofur/blob/main/NOTICE).
The name "Sermofur" is not licensed under Apache-2.0 (section 6).
