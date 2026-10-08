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
| entry whose attributes or listing cannot be read, or whose presence cannot be determined | `invalid_instance` (3), unreadable entry |
| link or junction, dangling or not | `unsafe_path` (4) |

The entry is inspected without following links. `doctor` is the only command that runs on an
invalid entry (except a link), to report it in its `instance` check. `init` never adopts nor
overwrites an entry. Within a listable instance, an `instance.json` that cannot be read is a
storage error, not an unreadable entry: every command that reads the configuration (all except
`root` and `doctor`) fails with `invalid_storage_or_path` (exit 3), and `doctor` reports
`invalid_storage` in its `instance` check (exit 5).

## Alternatives
Skip foreign entries and stop only on damaged instances: rejected, the classification can be
fooled by permissions or an emptied folder, and the failure mode is a silent write elsewhere.
Rely on the marker name alone: a distinctive name makes a foreign entry unlikely, but does not
remove the damaged and unreadable cases.

## Consequences
A `.sermofur` entry that belongs to another tool blocks Sermofur below it, with an explicit
message; the user renames or moves it, or works from another directory. No command ever falls
through to an ancestor instance.

Since 0.2, discovery refuses an entry owned by another account
([ADR 0014](0014-instance-owner.md)); the paragraphs below describe the 0.1 limit it closes.
Discovery walks up to the root of the volume and does not check who owns the entry it finds.
Sermofur assumes a single operating-system user. On a machine shared with other users, another
user can create a `.sermofur` entry in any folder above your working directory that they can
write to. On Windows, every authenticated user can create a folder at the root of a drive
(`C:\`) by default, and the folders created at that root, on an NTFS data volume or on FAT/exFAT
media are writable by every authenticated user. On Unix, anyone can create an entry in `/tmp`.
Such an entry blocks discovery and `init` below it, receives the memory you write from a folder
that none of your instances contains, or feeds you claims and RETEX that the other user wrote;
an instance kept in such a folder can also be read or changed by them. Since `init` and any
command run outside your instances walk up to the root of the drive, keeping your files in your
profile is not enough on its own. Until an ownership check exists, on a shared machine:

- keep your instance under a folder that only you can write to, such as your Windows profile or
  your Unix home;
- run `smf` only from inside that instance, and check with `smf root` that the root found is
  yours before writing;
- if `init` reports an entry at the root of the drive, check who owns it and remove it before
  running `init` again.
