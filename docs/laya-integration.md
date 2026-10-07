English | [Français](fr/laya-integration.md)

# Laya integration — design, not delivered

Preferred provider: https://github.com/NandhaKishorM/laya. Not a dependency of Domain.
Typed decisions port: worth_retaining / is_correction / is_novel / relevance / conflict /
validation / needs_system_two / consolidation / stale. Suggestions only, never rights or truth.

Planned: a managed Python runtime, a versioned model with checksum and license, explicit
download, lazy load and unload, local inference, timeout and validated typed output. One runtime
per machine, separate instance memories. Offline after installation. Laya missing → warning and
the host's System 2. No Laya inference is run and nothing is installed in 0.1.

Planned dataset format (specified outside the repository, see [AGENTS.md](../AGENTS.md)):
prediction + correction + validation/provenance, redacted or hashed text, model version, consent
scope. Opt-in export; no automated fine-tuning in V1.
