English | [Français](../fr/adr/0010-fail-closed-instance-discovery.md)

# ADR 0010-fail-closed-instance-discovery — Fail-closed instance discovery

Date: 2026-10-07. Status: accepted, implemented in 0.1. Completes
[ADR 0001](0001-storage.md).

## Context
Commands find their instance by walking up from the working directory to the nearest `.sermofur`
entry. That entry is not always a valid instance: a folder copied by hand, another tool's data,
a damaged or unreadable instance, or one replaced by a link. Skipping entries that do not look
like Sermofur data leaves gaps (an unreadable folder, an emptied folder, a dangling link) where a
command would silently write into an ancestor instance, that is into another memory.

## Decision
Discovery stops at the nearest `.sermofur` entry, whatever it is. A valid instance (a folder that
holds `instance.json`) is used. Any other entry fails closed:

| Entry | Result |
|---|---|
| file, or folder without any Sermofur artifact | `invalid_instance` (3), "rename or move it away" |
| `memory.db` or `records/` without `instance.json` | `invalid_instance` (3), damaged instance |
| attributes or content that cannot be read, including when the presence of the entry cannot be determined | `invalid_instance` (3), unreadable entry |
| link or junction, dangling or not | `unsafe_path` (4) |

The entry is inspected without following links. `doctor` is the only command that runs on an
invalid entry (except a link), to report it in its `instance` check. `init` never adopts nor
overwrites an entry.

## Alternatives
Skip foreign entries and stop only on damaged instances: rejected, the classification can be
fooled by permissions or an emptied folder, and the failure mode is a silent write elsewhere.
Rely on the marker name alone: a distinctive name makes a foreign entry unlikely, but does not
remove the damaged and unreadable cases.

## Consequences
A `.sermofur` entry that belongs to another tool blocks Sermofur below it, with an explicit message;
the user renames or moves it, or works from another directory. No command ever falls through to
an ancestor instance.
