English | [Français](fr/verification.md)

# Verification

How to check a build of Sermofur yourself, what the automated tests cover, and the known limits of
version 0.2.

## Check it yourself

From the repository root, with the .NET 10 SDK (the CI runs the same steps on Windows, Linux and
macOS for every pull request):

```powershell
dotnet tool restore                 # CSharpier, pinned in .config/dotnet-tools.json
dotnet restore --locked-mode        # fails if a dependency differs from the lock files
dotnet build --no-restore           # expected: 0 warnings, 0 errors (warnings are errors)
dotnet test --no-restore            # expected: every test passes
dotnet csharpier check .            # expected: no formatting difference
```

Package and install the tool locally, then run it:

```powershell
dotnet pack src/Sermofur.Cli -c Release -o artifacts/packages --no-restore
dotnet tool install Sermofur --version 0.0.0-dev --tool-path artifacts/tools --configfile nuget.local.config --no-cache
./artifacts/tools/smf --help      # expected: exit 0, first line "Sermofur 0.0.0-dev"
```

Try it on a disposable instance, never on a folder that holds real memory:

```powershell
mkdir $env:TEMP/sermofur-check; cd $env:TEMP/sermofur-check
<path-to>/smf init --json         # "created": true
<path-to>/smf claim add "sample claim" --origin user --json
<path-to>/smf doctor              # Overall: healthy_with_warnings, exit 0
```

`healthy_with_warnings` is the expected state of a sound instance: the capabilities that are not
delivered yet (daemon, laya, model, mcp) are reported as warnings.
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
- doctor: read-only behavior, `scope_overlap`, `scope_mappings`, identity mismatch, `search_index`
  out of sync, FTS5 available.
- Migration (ADR 0012): a format 1 instance built by the published 0.1.1 tool (test fixture) keeps
  every record, history row, idempotency key, scope and projection; claims and RETEX get indexed;
  the backup is a consistent format 1 database; other commands refuse format 1 without writing;
  an interruption after the backup, inside the transaction or before `instance.json` leaves a
  usable instance that `migrate` completes; `migrate` is idempotent.
- Sources: hash of exactly the bytes read, BOM accepted, empty, binary, Latin-1, oversized,
  missing and folder refused with stable codes; a file of a narrower scope, of another client,
  outside the instance or in `.sermofur` refused; idempotent add; reindex reporting unchanged,
  modified (previous hash in history), missing and restored, with the index following; reindex
  reading only the declared sources of the current scope; evidence freezing the source hash;
  index rebuild after a desynchronized index, reading every source again with history under the
  system actor, dropping a lost source and rolling back entirely when a read fails, counting
  visible objects only; a source of an ancestor moved to a scope created on its folder; a scope
  created during a source add seen under the write lock; one file is one source whatever the case
  typed, the file system deciding (case and NFC/NFD); a scope mapping typed in another case
  stored as named on disk, doctor warning on a stored variant; `.sermofur` unreachable through a
  case variant; doctor reporting a misplaced index entry and a source left in a broader scope; a
  file reached through a link refused; a FIFO refused without blocking (Linux and macOS).
- Recall (ADR 0013): at most 3 explained results in a stable order; content added to a sibling
  scope changes neither presence, order nor score of visible results; invalidated claims never
  first, counted as excluded; best passage and freshness of sources; query syntax treated as text;
  limit bounds; a real process on a read-only store writing nothing. The query tokens equal the
  index tokens (accents, CJK, emoji, separators).
- Challenge: independent contradiction capping confidence at medium, LLM contradiction noted
  without refuting, format 1 evidence read as support, source changed then gone, invalidated and
  review due, at most 3 close claims never called contradictions with nothing written, a claim of
  another client answering like an unknown one.
- Owner (ADR 0014): an entry of another account refused at discovery and named by doctor, own
  entries recognized, `instance.json` over 64 KiB damaged; on Linux and macOS a real entry owned by
  root through passwordless sudo, owner and group changed separately (fails if sudo is not
  available); on an elevated Windows session, an entry owned by SYSTEM refused and one owned by
  Administrators accepted.
- Properties (FsCheck): mapping normalization stays inside the root, is stable and portable
  across separators; the command-line grammar keeps every argument after `--` positional and never
  takes an option value starting with `--`.

## Recall performance

Reference measure of the spec (SC-004), `RecallPerformanceTests`, run with
`SERMOFUR_PERFORMANCE=1 dotnet test -c Release --filter Category=Performance`: 10,000 claims and
RETEX (half of the claims with evidence, some of it contradicting) plus 1,000 sources of about
20 KiB (11,000 objects, 20,000 indexed passages), 30 questions of three frequent terms, each on a
newly opened read-only store, as a CLI command does. On 2026-10-08, Windows 11, Intel Core
i7-1255U, 32 GB: **p50 346 ms, p95 414 ms, max 443 ms**, under the 1 s ceiling of the spec. The
300 ms p95 target set by the 0.1 plan is not reached: most of the time goes to reading the term
frequencies of the vocabulary table and the visible passages.

## Known limits

- Platforms: the CI builds and runs the tests on Windows, Linux and macOS for every pull request
  and push to `main`; manual checks (console code page, network drive) were done on Windows.
- Console code page: switching to UTF-8 and restoring the original code page are not tested
  automatically, because tests redirect both streams and that branch then does not run. Restoring
  on normal exit and on a handled error was checked by hand on Windows (cmd.exe, `chcp` before and
  after). Restoring on Ctrl+C was not checked.
- Discovery: a `.sermofur` entry counts as an instance only if it is a directory holding
  `instance.json`; any other entry blocks Sermofur below it. The unreadable case is tested on
  Windows only. Mappings are compared lexically (no canonicalization of case, 8.3 aliases or
  Unicode normalization). The owner check is verified on Windows, Linux x64 and macOS arm64;
  macOS x64 is not.
- On Unix, the publishing rename of `init` does not detect an empty `.sermofur` created in the
  instant before it; this race is not tested.
- A mapped network drive is refused by the code but this was not tested, for lack of such a
  drive.
- Not delivered, hence not verified: MCP protocol, Laya inference, daemon IPC, UI, self-contained
  installer, consolidation, semantic similarity. No power loss was simulated.
- Recall performance (SC-004) is a reference measure, not a guarantee: see [Recall performance](#recall-performance). It runs only
  with `SERMOFUR_PERFORMANCE=1` and is not part of the CI.
- The tool package requires the .NET 10 runtime.
