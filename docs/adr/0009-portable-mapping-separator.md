English | [Français](../fr/adr/0009-portable-mapping-separator.md)

# ADR 0009-portable-mapping-separator — Portable separator of stored mappings

Date: 2026-10-07. Status: accepted, implemented in 0.1. Supersedes in part
[ADR 0002](0002-scopes.md).

## Context
Scope mappings are stored relative to the instance root. With the native separator, the same
instance would read differently depending on the operating system, and a folder synchronized or
copied between Windows and Unix would lose its mappings. Windows users also type `\` naturally.

## Decision
Mappings are stored with `/` on every operating system. Input and reading accept `\` as a
separator too, on every operating system: a mapping typed on Windows, or a row holding `\`, is
read the same way everywhere.

## Alternatives
Native separator per operating system: an instance would not read the same everywhere.
Accept `\` only as Windows input: the stored form would stay portable, but a row written with
`\` by another tool or by hand would read differently on Unix.

## Consequences
On Unix, a `\` in a directory name is read as a separator: such a directory cannot be mapped.
This is a deliberate portability choice; such names are rare and are not portable to Windows
anyway.
