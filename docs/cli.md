English | [Français](fr/cli.md)

# CLI 0.1

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
| status | Identity, scope, visible counts, bootstrap mode |
| doctor | Integrity, schema, foreign keys, scopes, mappings, overlaps, consistency, projections; no mutation |
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

`--origin user|llm` is **mandatory** on `claim add`, `evidence add` and `retex add` only — not
on `scope add` — with no default value: when missing, the result is `invalid_arguments` (exit 1)
and nothing is written. The origin stays declarative until the host channel (daemon/MCP) sets it.
Other `add` options: `--actor` (default `local-user`), `--key` for idempotency.
Claim: `--category episodic|semantic|procedural|preferences|decisions`;
`--volatility stable|evolving|volatile`.
Evidence kinds: `execution`, `source_code`, `authoritative_documentation`, `project_decision`,
`local_documentation`, `human_observation`, `user_assertion`, `llm_assertion`.
A `preferences` or `decisions` claim requires `--origin user`. Declared `execution` evidence
stays low in this version. There is no `--scope` argument: the scope comes from the path.
Unknown options are refused before any mutation. No `--quiet`/`--verbose` yet.

Command-line bounds: at most 128 arguments, each at most 16,384 characters and without NUL.
They are checked before any command, hence before any service and any write; exceeding them
gives `invalid_arguments` (exit 1).

`--help` is recognized anywhere **before** `--` and prints the usage (exit 0) before any instance
lookup and before the bounds check; after `--`, it is a positional argument like any other.
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
`records/` without `instance.json`) or an unreadable entry (attributes or content that cannot be
read). A link or junction, even dangling, is refused with `unsafe_path` (exit 4) by every command,
`doctor` included. Nothing is ever written into an instance higher up. For a damaged instance,
run `smf doctor` from that folder, then restore `instance.json` from a backup; for a foreign
entry, rename or move it away. `doctor` names the case in its `instance` check:
`invalid_instance: foreign`, `invalid_instance: damaged` or `invalid_instance: unreadable`.

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
`scope_mappings` warning, with only the number of missing mappings.

| Exit | Meaning |
|---|---|
| 0 | Success, help, or doctor healthy with warnings |
| 1 | Invalid input / not_found / idempotency conflict / duplicate mapping |
| 2 | No instance |
| 3 | Storage/version/permissions/projection to rebuild, invalid `.sermofur` entry (foreign, damaged, unreadable) |
| 4 | Scope/path boundary, nested instance |
| 5 | Doctor unhealthy |

The 0.1 CLI calls Application directly. runtime/mcp/config/source/recall/challenge are in the
backlog, never empty commands that report a success.
