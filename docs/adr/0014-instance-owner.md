English | [Français](../fr/adr/0014-instance-owner.md)

# ADR 0014-instance-owner — An instance of another account is never read

Date: 2026-10-08. Status: accepted, implemented in 0.2. Completes
[ADR 0010](0010-fail-closed-instance-discovery.md).

## Context
ADR 0010 left the owner of a `.sermofur` entry unchecked: on a shared machine, another account can
create an entry above your working directory, receive the memory you write or feed you its own.
Indexed sources make this worse, since the index holds the text of your files.

## Decision
- Discovery checks the owner of the `.sermofur` entry it finds, before reading it. An entry of
  another account fails with `foreign_owner` (exit 4); `doctor` names the case
  `invalid_instance: foreign_owner`.
- Windows: the owner SID must be the current user, or the Administrators group when the current
  user belongs to it (folders created from an elevated session).
- Linux: `statx` without following links; macOS: `lstat` with 64-bit inodes (`lstat$INODE64` on
  x64). The owner UID is compared with `geteuid()`. .NET exposes no file owner on Unix; these
  structures are stable ABIs, unlike glibc's `stat` wrappers.
- The same call gives the type of a source file: a FIFO, device or socket is rejected instead of
  being opened.

## Alternatives
Running `stat` and `id`: an external process found through `PATH`. Mono.Posix: unmaintained.
A probe through `SetUnixFileMode`: a write, forbidden to `doctor`.

## Consequences
The check runs on Windows, Linux x64 and macOS arm64 in CI, the real case "entry owned by root"
through passwordless sudo on the runners. macOS on x64 is not verified. The check does not replace
file-system permissions: it refuses to read, it does not protect files others can write to.
