English | [Français](fr/scope-model.md)

# Scope model

workspace → client → project → repository → task. One tree per instance. IDs are unique slugs.
Visibility includes the current scope and its ancestors only; it excludes descendants,
siblings and other clients. The workspace is not an administrator access to client memories.
doctor checks the instance structures globally but returns no business content.

Path mappings are explicit; client names are never inferred by heuristics. Without a mapping,
the context is the workspace. The longest path that contains the context wins. A project and its
child repository may share a real path; the repository wins the tie. Siblings do not share a
mapping.
No overlap between branches: a mapping can neither contain nor be contained in the mapping of a
scope that is neither its ancestor nor its descendant. Two clients therefore never nest, and the
longest prefix can never slide a context from one client to another.
Links, junctions and network paths (UNC as well as mapped network drives on Windows) are refused
in this version. A mapped directory that disappeared only blocks its own context; doctor reports
it as a warning. These rules are replayed under the write transaction of `scope add`, and doctor
checks them on the stored tree (`scope_overlap`).

Accepted consequence in 0.1: a project that shares the path of its repository is no longer
reachable as a write context once the repository is registered, since the repository wins the
tie. Writing at the project level then requires a mapping distinct from the repository's.

Known limit: mappings are compared lexically (normalized paths, no disk access). Windows 8.3
aliases are not canonicalized, nor is case: the comparison ignores case on Windows but respects
it elsewhere, including on a case-insensitive file system. Unicode normalization forms (NFC and
NFD, as on macOS) are not unified either. Two spellings of the same folder can therefore escape
the duplicate or overlap check.

Example for an existing single-repository project, without creating folders:
```powershell
smf --path C:\work scope add acme client workspace clients/acme
smf --path C:\work\clients\acme scope add billing project acme clients/acme/billing-api
smf --path C:\work\clients\acme\billing-api scope add billing-repo repository billing clients/acme/billing-api
```
All mapping paths are relative to the instance root, even from a subfolder. They are stored with
forward slashes ([instance format](instance-format.md)).
This first vertical slice does not select a logical task outside a mapping; that contract will
come with daemon sessions. Multiple or disjoint contexts per scope are in the backlog.

Evidence lives with its claim. Mutating an ancestor from a child is refused; moving to the
parent context expresses the intent explicitly. Cross-client promotion is not implemented: no
raw memory flows from A to B, not even through a lineage. A future generalization will keep the
sources in their scope and expose to other clients only a sanitized, validated result.
