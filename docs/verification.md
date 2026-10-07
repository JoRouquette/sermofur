English | [Français](fr/verification.md)

# Verification

How to check a build of Sermofur yourself, what the automated tests cover, and the known limits of
version 0.1.

## Check it yourself

From the repository root, with the .NET 10 SDK and CSharpier installed:

```powershell
dotnet restore --locked-mode        # fails if a dependency differs from the lock files
dotnet build --no-restore           # expected: 0 warnings, 0 errors (warnings are errors)
dotnet test --no-restore            # expected: every test passes
csharpier check .                   # expected: no formatting difference
```

Package and install the tool locally, then run it:

```powershell
dotnet pack src/Sermofur.Cli -c Release -o artifacts/packages --no-restore
dotnet tool install Sermofur --version 0.1.0 --tool-path artifacts/tools --configfile nuget.local.config
./artifacts/tools/smf --help      # expected: exit 0, first line "Sermofur 0.1"
```

Try it on a disposable instance, never on a folder that holds real memory:

```powershell
mkdir $env:TEMP/sermofur-check; cd $env:TEMP/sermofur-check
<path-to>/smf init --json         # "created": true
<path-to>/smf claim add "sample claim" --origin user --json
<path-to>/smf doctor              # Overall: healthy_with_warnings, exit 0
```

`healthy_with_warnings` is the expected state of a sound 0.1 instance: the capabilities that are
not delivered yet (daemon, laya, model, mcp, indexes, contradictions) are reported as warnings.
`artifacts/` and `TestResults/` are ignored by Git.

## What the tests cover

- Domain: scope tree, kind hierarchy, visibility of ancestors only (generated trees included),
  cycles and invalid parents.
- Storage: schema initialization, foreign keys (application check), persistence and history
  across connections, idempotency (same key, changed content, concurrent replays), future schema
  and corrupt database refused, projection lost or impossible after commit then rebuilt by
  export.
- Isolation: client A / client B, separate instances, forged visibility refused by the service,
  evidence bound to the scope of its claim, cross-client errors without content.
- Scopes: nested clients in both directions, a project in another client's tree (including through
  `..`), overlapping sibling projects, taken ID with the same message whoever owns it, identical
  mapping, invalid slugs and kinds, parent outside the context, NUL and overlong mappings, a
  concurrent `scope add` simulated just before the transaction, a deleted mapped folder, mappings
  stored with forward slashes, backslash mappings in the store still read, and a forward-slash mapping
  refused as a duplicate of the same stored backslash mapping.
- Discovery (fail-closed, ADR 0010): nearest instance, nested `init` refused, a foreign
  `.sermofur` entry in the target directory or in a parent stopping discovery and `init` and left
  untouched, a damaged child instance stopping discovery (no `instance.json`, only `memory.db`,
  only `records/`) and, through the CLI, `init`, `root`, `status`, `export`, `scope list` and
  `claim add` with nothing written in the ancestor instance, an unreadable `.sermofur` entry
  (deny ACE, Windows only) stopping discovery, a `.sermofur` link refused, dangling or not (a
  junction on Windows), including by `doctor`.
- doctor on an invalid entry: `invalid_instance: foreign`, `invalid_instance: damaged` and
  `invalid_instance: unreadable` in the `instance` check, exit 5, nothing written in the ancestor
  instance; for foreign and damaged entries, also nothing written in the entry nor in the start
  folder when doctor starts from a subfolder of the folder that holds the entry.
- CLI: real processes and in-process runs, JSON output and exit codes, mandatory `--origin`,
  `--help` anywhere, `--` separator, command-line bounds (128 arguments, 16,384 characters, NUL),
  bounded random input on the `claim show` identifier (fixed seed), UTF-8 stdout with non-ASCII
  text and stderr without BOM (ASCII messages) in a real process, accents kept literal while
  backticks and HTML stay escaped.
- doctor: read-only behavior, `scope_overlap`, `scope_mappings`, identity mismatch.

## Known limits

- Platforms: Windows is verified first. Linux and macOS are not verified yet; no remote CI is
  configured.
- Console code page: switching to UTF-8 and restoring the original code page are not tested
  automatically, because tests redirect both streams and that branch then does not run. Restoring
  on normal exit and on a handled error was checked by hand on Windows (cmd.exe, `chcp` before and
  after). Restoring on Ctrl+C was not checked.
- Discovery: a `.sermofur` entry counts as an instance only if it is a directory holding
  `instance.json`; any other entry blocks Sermofur below it. The unreadable case is tested on
  Windows only. Mappings are compared lexically (no canonicalization of case, 8.3 aliases or
  Unicode normalization). Discovery does not check who owns a `.sermofur` entry (ADR 0010).
- On Unix, the publishing rename of `init` does not detect an empty `.sermofur` created in the
  instant before it; Unix is not verified.
- A mapped network drive is refused by the code but this was not tested, for lack of such a
  drive.
- Not delivered, hence not verified: MCP protocol, Laya inference, daemon IPC, UI, self-contained
  installer, indexing/FTS/recall/challenge, consolidation, schema migrations beyond v1. No power
  loss was simulated.
- The tool package requires the .NET 10 runtime.
