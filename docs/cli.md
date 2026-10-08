English | [Français](fr/cli.md)

# CLI 0.4

`smf [--path DIRECTORY] [--json] COMMAND ...`. The path is an existing local directory, the
current directory by default; it designates the working context, never an arbitrary scope.

Output: with `--json`, results are JSON on stdout and errors are JSON `{code, message}` on stderr
only, with stdout empty on error. Human output: `root` as text, objects as indented JSON, `doctor`
as a list of checks. Error codes and exit codes are stable; messages are short English texts
that may change.

stdout and stderr are written in UTF-8 without BOM, whatever the console code page. The console
code page is switched to UTF-8 only when stdout or stderr reaches the console; when both are
redirected it is left untouched. `smf` restores the original code page on normal exit, on
handled errors and on Ctrl+C: the caller's console is not changed durably. Restoration is not
guaranteed on abrupt termination (killed process, closed window), nor when several `smf`
processes share the same console at once (for example `smf … | smf …`). A caller that
captures the output reads it as UTF-8; in Windows PowerShell, run first
`[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)`, otherwise PowerShell
decodes the captured output with the OEM code page and corrupts accented characters.
JSON is readable UTF-8: accented letters stay literal; `<`, `>`, `&` and the backtick stay
escaped, so a text cannot close the JSON block of a projection.

| Command | Result |
|---|---|
| init | New instance, or the same identity if it already exists; no nested instance |
| root | Nearest instance going up (a `.sermofur` directory holding `instance.json`) |
| status | Identity, scope, visible counts; `mode` is `daemon` when the daemon ran the command, `direct` otherwise |
| doctor | Integrity, schema, foreign keys, scopes, mappings, overlaps, consistency, projections, state of the daemon; no mutation |
| scope current | Current scope and visible ancestors |
| scope list / tree | Visible scopes only (tree = list in 0.1) |
| scope add ID KIND PARENT RELATIVE_PATH | Direct child of the current scope; typed immediate parent |
| claim add TEXT --origin user\|llm | Proposed claim, confidence computed when read |
| claim list / show ID | Visible objects / the claim with its evidence and history |
| claim invalidate ID --reason TEXT | Auditable invalidation of a claim of the current scope |
| evidence add CLAIM_ID KIND REFERENCE --lineage ORIGIN --origin user\|llm | Declared evidence, same scope as the claim |
| evidence list / show ID | Visible evidence |
| retex add --event TEXT --impact TEXT --next TEXT --origin user\|llm | Draft RETEX, no implicit learning |
| retex list / show ID | Visible RETEX |
| export | Rebuilds the projections of the visible objects |
| migrate | Instance format 1 (0.1) → 2: backup, then migration in one transaction; nothing to do on format 2 |
| source add FILE --origin user\|llm | Declared source of the current scope: hashed, indexed, idempotent |
| source list / show ID | Visible sources / the source with its history |
| source reindex [ID] | Reads again the sources of the current scope: unchanged, modified, missing, unreadable, rejected, restored, or skipped when another command changed the source meanwhile |
| index rebuild | Rebuilds the full-text index of the instance in one write transaction and reads every source again (same outcomes as `source reindex`, history under the system actor `sermofur`); other commands wait up to 5 s, then fail with `storage_busy` (exit 3) and are to be run again; output `{indexed, changedSources}` counts visible objects only |
| recall QUESTION [--limit 1-3] | At most 3 explained results among visible claims, RETEX and source passages |
| challenge CLAIM_ID / challenge --text TEXT | Contradictions, changed sources, status, review date, close claims to confront; writes nothing |
| daemon install / uninstall / start / stop / restart / status | The daemon service of your session; see [daemon.md](daemon.md) |
| daemon register / unregister / instances | Instances the daemon may serve |
| daemon run [--supervise] | The daemon in the foreground |

When the daemon runs and serves the instance, every command except `daemon …`, `init`,
`doctor`, `--help` and `--version` goes through it, with the same output, errors and exit codes;
otherwise the CLI runs it directly. `SERMOFUR_NO_DAEMON=1` forces direct runs. A daemon of
another version stops the command with `daemon_version_mismatch` (exit 3) until
`smf daemon restart`. The other codes of the daemon are listed in [daemon.md](daemon.md#errors).

`--origin user|llm` is **mandatory** on `claim add`, `evidence add`, `retex add` and
`source add` — not on `scope add` — with no default value: when missing, the result is
`invalid_arguments` (exit 1) and nothing is written. The origin stays declarative until the host channel (daemon/MCP) sets it.
Other `add` options: `--actor` (default `local-user`), `--key` for idempotency (not on
`source add`, which is idempotent by path).
Claim: `--category episodic|semantic|procedural|preferences|decisions`;
`--volatility stable|evolving|volatile`.
Evidence kinds: `execution`, `source_code`, `authoritative_documentation`, `project_decision`,
`local_documentation`, `human_observation`, `user_assertion`, `llm_assertion`.
A `preferences` or `decisions` claim requires `--origin user` (otherwise `user_choice_required`,
exit 1). Declared `execution` evidence
stays low in this version. `evidence add` also takes `--contradicts` (evidence against the claim,
a flag without value) and `--source SOURCE_ID` (a visible indexed source; its hash at that moment
is kept with the evidence). There is no `--scope` argument: the scope comes from the path.
Unknown options are refused before any mutation. No `--quiet`/`--verbose` yet.

Command-line bounds: at most 128 arguments, each at most 16,384 characters and without NUL.
They are checked before any command, hence before any service and any write; exceeding them
gives `invalid_arguments` (exit 1).

`--help` (`-h`) and `--version` (`-v`) are recognized anywhere **before** `--`, except as the value of an option (`--actor -v`
keeps the actor `-v`), and answer (exit 0)
before any instance lookup and before the bounds check; after `--`, they are positional arguments
like any other, so a text that is `-h` or `-v` goes after `--`. `--help` wins when both are given.
`smf -h` prints the general usage; `smf COMMAND -h` (`smf claim add -h`, `smf recall -h`…) prints
the help of that command: synopsis, description, arguments, options with their values and
defaults, its own error codes and an example; `smf GROUP -h` (`smf source -h`) lists the
subcommands of a group. `--version` prints `Sermofur <version>`, or `{"version": "<version>"}`
with `--json`.
`--` ends options: every following argument is positional, which allows a text starting with
`--` (`smf claim add --origin user -- "--text"`).
An option value cannot start with `--`: `--actor --x` gives `invalid_arguments` (exit 1). Only a
positional argument placed after `--` may start with two dashes.

## init

| Code | Exit | Case |
|---|---|---|
| `invalid_path` | 1 | The directory does not exist |
| `nested_instance` | 4 | The nearest `.sermofur` entry above is a valid instance |
| `invalid_instance` | 3 | The nearest `.sermofur` entry, in the target directory or above, is not a valid instance (foreign, damaged or unreadable); it is left untouched |
| `unsafe_path` | 4 | Network path (UNC or mapped network drive), link or junction, including a `.sermofur` link |

Discovery fails closed ([ADR 0010](adr/0010-fail-closed-instance-discovery.md)): it stops at the
nearest `.sermofur` entry, whatever it is. If that entry is not a valid instance, every command
except `doctor` fails with `invalid_instance` (exit 3) for a foreign entry (a file, or a folder
holding none of `instance.json`, `memory.db`, `records/`), a **damaged instance** (`memory.db` or
`records/` without `instance.json`) or an unreadable entry (attributes or listing that cannot be
read). A link or junction, even dangling, is refused with `unsafe_path` (exit 4) by every command,
`doctor` included. Nothing is ever written into an instance higher up. For a damaged instance,
run `smf doctor` from that folder, then restore `instance.json` from a backup; for a foreign
entry, rename or move it away. `doctor` names the case in its `instance` check:
`invalid_instance: foreign`, `invalid_instance: damaged`, `invalid_instance: unreadable` or
`invalid_instance: foreign_owner`.

A `.sermofur` entry that belongs to another user account is never read: every command but
`doctor` fails with `foreign_owner` (exit 4) ([ADR 0014](adr/0014-instance-owner.md)). An
`instance.json` larger than 64 KiB is a damaged instance (`invalid_instance`, exit 3).

## migrate

The 0.2 tool reads instances of format 2. On a format 1 instance (created by 0.1), every command
except `migrate`, `doctor` and `root` fails with `migration_required` (exit 3) and changes nothing;
`doctor` reports `migration_required`. `smf migrate` checks the consistency of the database,
backs it up with the SQLite backup API to `.sermofur/backups/memory-v1-<UTC time>-<id>.db`,
migrates it in one transaction (objects, history, idempotency keys and projections unchanged;
claims and RETEX indexed), then replaces `instance.json`. If it is interrupted, run it again: it
resumes where it stopped. Output: `{migrated, from, to, backup}`; on a format 2 instance,
`migrated: false` ([ADR 0012](adr/0012-instance-format-2.md)).

## Sources

`source add FILE` declares a local text file as a source of the current scope. A relative `FILE`
is relative to the context directory (`--path`, the current directory by default). The file must
be in the directory of the current scope (the instance root for the workspace) and outside the
directory of any narrower scope: a file of a client is added from that client's directory, so
that it never becomes visible to sibling scopes. The file is read once, at most 1 MiB, as strict
UTF-8 (a BOM is accepted); its SHA-256 hash covers exactly the bytes indexed. Its text is split
into passages of at most 2,000 characters. Adding the same file again returns the same source.

`source reindex` reads again only the declared sources of the current scope; no other file is
ever read, and `recall`/`challenge` read no file at all. The previous hash stays in the history.
When `scope add` creates a scope whose directory holds sources of an ancestor, those sources move
to the new scope in the same transaction (history "rescoped"), so that they never stay visible to
its siblings; `doctor` reports a source attached to a broader scope than its file
(`source_scopes`). A source is stored under the name its file has on disk: on a case-insensitive file system
(Windows, macOS by default), `Doc.md` and `doc.md` are one source.
A missing, unreadable or rejected source leaves the index, its record is kept.

| Code | Exit | Case |
|---|---|---|
| `source_rejected` | 1 | Empty, binary (NUL byte, invalid UTF-8, special file) or larger than 1 MiB; the message gives the reason |
| `invalid_path` | 1 | Missing file, folder or unreadable file |
| `scope_boundary` | 4 | Outside the instance, outside the directory of the current scope, inside a narrower scope, or already a source of another scope |
| `unsafe_path` | 4 | Link, junction, network path, or a file of `.sermofur` |
| `source_unavailable` | 1 | `evidence add --source` on a missing, unreadable or rejected source |

## recall and challenge

`recall` treats the question as plain text (no query syntax), ignoring case and accents. It
returns at most 3 results, each with its kind, identifier, scope, status, confidence and reasons
(claims), the matching excerpt, the matched terms and its freshness (creation, last verification
or review date of a claim, last indexing and hash of a source). Only the current scope and its
ancestors are searched, and the ranking statistics are computed on them only, so that content of
another scope changes neither presence nor order ([ADR 0013](adr/0013-filtered-ranking.md)). An
invalidated or superseded claim is never presented as applicable: it only fills remaining places,
marked `applicable: false`; `excluded` counts the visible ones left out.

`challenge CLAIM_ID` returns signals — `contradiction`, `source_changed` (with the recorded and
current hashes), `source_unavailable`, `not_applicable`, `review_due` — and at most 3 close claims
`toConfront`, never called contradictions. `challenge --text TEXT` does the same for a text that
is not a claim yet. Neither writes anything. An identifier of another scope answers like an
unknown one.

## scope add

| Code | Exit | Case |
|---|---|---|
| `invalid_arguments` | 1 | Command-line bound exceeded (see above): refused before the scope service |
| `invalid_scope` | 1 | Invalid slug (`[a-z][a-z0-9-]{0,63}`), identifier or mapping longer than 16,384 characters or containing NUL (a service check only: unreachable from the CLI, which refuses earlier with `invalid_arguments`; it protects the future daemon/MCP channel), unknown kind or kind incompatible with the parent, empty mapping, identifier already taken — same message whoever owns the scope, so that a sibling scope is not revealed |
| `duplicate_mapping` | 1 | Path already mapped identically by another scope (except a repository sharing its project's path) |
| `invalid_path` | 1 | Mapped directory does not exist |
| `scope_boundary` | 4 | Parent other than the current scope, mapping outside the parent or the instance, or mapping that contains or is contained in one of another branch |
| `unsafe_path` | 4 | Absolute mapping or mapping with a drive, link, junction, network path (UNC or mapped network drive) |

The mapping is stored relative to the instance root with forward slashes (`a/b`), whatever the
operating system; `\` is also accepted as a separator on input and when reading
([ADR 0009](adr/0009-portable-mapping-separator.md)).

Transactional revalidation: the candidate mapping and the parent are checked physically before
writing; the free identifier, the duplicate and the overlap are replayed on the scopes read again
under the `BEGIN IMMEDIATE` transaction of the insertion. Two concurrent `scope add` cannot
therefore register two incompatible mappings: the second is refused with the code above and
nothing is written. `doctor` applies the same rules (`scope_overlap`, error, with only the number
of conflicting pairs) to detect an overlap entered outside the CLI.

A mapped directory that was deleted does not prevent other contexts from working: only the
mapping selected for the current path is checked physically. `doctor` reports it as a
`scope_mappings` warning, with only the number of missing mappings. The same warning counts the
mappings stored by an earlier version under a spelling that the file system resolves but that does
not compare equal to the name on disk (case outside Windows, Unicode normalization): such a scope
may not hold the sources of its folder, and Sermofur 0.2 offers no command to rewrite a mapping.

| Exit | Meaning |
|---|---|
| 0 | Success, help, or doctor healthy with warnings |
| 1 | Invalid input / not_found / idempotency conflict / duplicate mapping / instance not registered / request too large |
| 2 | No instance |
| 3 | Storage/version/permissions/projection to rebuild, invalid `.sermofur` entry (foreign, damaged, unreadable), migration required, daemon of another version or unavailable, service manager unavailable or refusing |
| 4 | Scope/path boundary, nested instance, entry or daemon endpoint of another account |
| 5 | Doctor unhealthy |

mcp and config are in the backlog, never empty commands that report a success.
