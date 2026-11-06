//! The commands of the memory of an instance, ported from `CommandRunner.Dispatch`,
//! `CommandRunner.RunScope` and `MemoryCommands.cs`: `status`, `scope`, `export`, `claim`,
//! `evidence` and `retex`. Each returns the JSON text of its answer.

use serde::Serialize;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::format::DateTimeOffset;
use sermofur_core::guid::{self, Uuid};
use sermofur_core::instance::InstanceConfiguration;
use sermofur_core::json;
use sermofur_core::memory::{MemoryContext, MemoryService};
use sermofur_core::records::{
    ActorKind, ClaimContent, DotNetEnum, EvidenceContent, EvidenceKind, EvidenceRelation,
    MemoryCategory, Provenance, RecordKind, RetexContent, Scope, ScopeKind, Volatility,
};
use sermofur_core::scopes;
use sermofur_core::store::Store;

use crate::args::CommandArguments;
use crate::runner::ConfigurationBody;

/// Default actor of the commands that write.
const LOCAL_USER: &str = "local-user";

/// `CommandRunner.Writes`: the commands that may write, for which the store is opened
/// read-write. The knowledge commands of format 2 (lots 4 and 5) are included.
pub fn writes(args: &CommandArguments) -> bool {
    let positionals = args.positionals();
    let action = positionals.get(1).map(String::as_str);
    match positionals[0].as_str() {
        "init" | "migrate" | "index" | "export" => true,
        "source" => matches!(action, Some("add" | "reindex")),
        "recall" | "challenge" => false,
        _ => matches!(action, Some("add" | "invalidate")),
    }
}

/// `KnowledgeCommands.IsKnowledgeCommand`.
pub fn is_knowledge_command(command: &str) -> bool {
    matches!(command, "source" | "index" | "recall" | "challenge")
}

/// `CommandArguments.ParseEnum<T>`: `_` and `-` removed, then the .NET name of a member,
/// without case; `type_name` names the enumeration in the message. The .NET guards
/// (`All(char.IsLetter)`, then `Enum.TryParse` with `OrdinalIgnoreCase` and `IsDefined`) accept
/// exactly these names: every name is ASCII letters, and `OrdinalIgnoreCase` folds no other
/// character onto one (`ſ`, `ı` and the Kelvin sign stay: `cli-3-5/soft_enums.json`).
pub fn parse_enum<E: DotNetEnum>(value: &str, type_name: &str, code: &str) -> Result<E> {
    let normalized: String = value.chars().filter(|c| *c != '_' && *c != '-').collect();
    E::NAMES
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(&normalized))
        .map(|(_, _, member)| *member)
        .ok_or_else(|| SermofurError::new(code, format!("Invalid {type_name} value."), 1))
}

/// `CommandArguments.ParseId`: a GUID that is not empty.
pub fn parse_id(value: &str) -> Result<Uuid> {
    guid::try_parse(value)
        .filter(|id| !id.is_nil())
        .ok_or_else(|| SermofurError::invalid_arguments("GUID identifier required."))
}

fn to_json<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    json::to_string(value).map_err(|_| {
        SermofurError::new(
            "invalid_storage_or_path",
            "Invalid path or data; run doctor.",
            3,
        )
    })
}

#[derive(Serialize)]
struct StatusBody<'a> {
    root: String,
    configuration: ConfigurationBody,
    scope: &'a str,
    claims: usize,
    evidence: usize,
    retex: usize,
    sources: usize,
    mode: &'a str,
    laya: &'a str,
}

#[derive(Serialize)]
struct ExportBody {
    files: Vec<String>,
}

/// `Dispatch`: runs a command of the memory and returns its JSON answer.
pub fn dispatch(
    args: &mut CommandArguments,
    store: &mut Store,
    context: &MemoryContext,
    configuration: &InstanceConfiguration,
) -> Result<String> {
    let command = args.positionals()[0].clone();
    match command.as_str() {
        "status" => {
            args.require_count(1)?;
            args.validate_used()?;
            let records = MemoryService::new(store, context)?.list(None)?;
            let count = |kind: RecordKind| records.iter().filter(|r| r.kind == kind).count();
            to_json(&StatusBody {
                root: context.root.to_string_lossy().into_owned(),
                configuration: ConfigurationBody::from(configuration),
                scope: &context.scope_id,
                claims: count(RecordKind::Claim),
                evidence: count(RecordKind::Evidence),
                retex: count(RecordKind::Retex),
                sources: count(RecordKind::Source),
                // ponytail: always direct until the daemon (lot 7.4) runs commands for clients.
                mode: "direct",
                laya: "unavailable",
            })
        }
        "scope" => run_scope(args, store, context),
        "export" => {
            args.require_count(1)?;
            args.validate_used()?;
            let files = MemoryService::new(store, context)?.export()?;
            to_json(&ExportBody {
                files: files
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect(),
            })
        }
        "claim" => run_memory(args, MemoryService::new(store, context)?, RecordKind::Claim),
        "evidence" => run_memory(
            args,
            MemoryService::new(store, context)?,
            RecordKind::Evidence,
        ),
        "retex" => run_memory(args, MemoryService::new(store, context)?, RecordKind::Retex),
        _ => Err(SermofurError::invalid_arguments("Unknown command.")),
    }
}

fn run_scope(
    args: &mut CommandArguments,
    store: &mut Store,
    context: &MemoryContext,
) -> Result<String> {
    if args.positionals().len() < 2 {
        return Err(SermofurError::invalid_arguments(
            "Scope subcommand required.",
        ));
    }
    let action = args.positionals()[1].clone();
    if action == "add" {
        args.require_count(6)?;
        args.validate_used()?;
        let p = args.positionals();
        let candidate = Scope {
            id: p[2].clone(),
            kind: parse_enum::<ScopeKind>(&p[3], "ScopeKind", "invalid_scope")?,
            parent_id: Some(p[4].clone()),
            relative_path: Some(p[5].clone()),
        };
        return to_json(&scopes::register(store, context, candidate)?);
    }
    args.require_count(2)?;
    args.validate_used()?;
    match action.as_str() {
        "current" => to_json(&scopes::current(store, context)?),
        "list" | "tree" => to_json(&scopes::list(store, context)?),
        _ => Err(SermofurError::invalid_arguments(
            "Unknown scope subcommand.",
        )),
    }
}

/// `MemoryCommands.Execute`.
fn run_memory(
    args: &mut CommandArguments,
    mut memory: MemoryService<'_>,
    kind: RecordKind,
) -> Result<String> {
    if args.positionals().len() < 2 {
        return Err(SermofurError::invalid_arguments(
            "Memory subcommand required.",
        ));
    }
    let action = args.positionals()[1].clone();
    match action.as_str() {
        "list" => {
            args.require_count(2)?;
            args.validate_used()?;
            to_json(&memory.list(Some(kind))?)
        }
        "show" => {
            args.require_count(3)?;
            args.validate_used()?;
            let id = parse_id(&args.positionals()[2])?;
            let record = memory.get(id)?;
            if record.kind != kind {
                return Err(SermofurError::new(
                    "not_found",
                    "Object missing or inaccessible.",
                    1,
                ));
            }
            if kind == RecordKind::Claim {
                to_json(&memory.explain(id)?)
            } else {
                to_json(&record)
            }
        }
        "invalidate" if kind == RecordKind::Claim => {
            args.require_count(3)?;
            let reason = args.required("reason")?;
            let actor = args.option_or("actor", LOCAL_USER);
            args.validate_used()?;
            let id = parse_id(&args.positionals()[2])?;
            to_json(&memory.invalidate(id, &reason, &actor)?)
        }
        "add" => add(args, memory, kind),
        _ => Err(SermofurError::invalid_arguments(
            "Unknown memory subcommand.",
        )),
    }
}

fn add(
    args: &mut CommandArguments,
    mut memory: MemoryService<'_>,
    kind: RecordKind,
) -> Result<String> {
    // The origin stays declarative but never implicit: no default value.
    let origin =
        parse_enum::<ActorKind>(&args.required("origin")?, "ActorKind", "invalid_arguments")?;
    let provenance = Provenance {
        origin,
        actor: Some(args.option_or("actor", LOCAL_USER)),
        timestamp: DateTimeOffset::now_utc(),
    };
    let key = args.option("key").map(str::to_owned);
    let key = key.as_deref();
    let record = match kind {
        RecordKind::Claim => {
            args.require_count(3)?;
            let category = parse_enum::<MemoryCategory>(
                &args.option_or("category", "semantic"),
                "MemoryCategory",
                "invalid_arguments",
            )?;
            let volatility = parse_enum::<Volatility>(
                &args.option_or("volatility", "evolving"),
                "Volatility",
                "invalid_arguments",
            )?;
            args.validate_used()?;
            let claim = ClaimContent {
                text: Some(args.positionals()[2].clone()),
                category,
                volatility,
                last_verified: None,
                review_after: None,
            };
            memory.create_claim(&claim, provenance, key)?
        }
        RecordKind::Evidence => {
            args.require_count(5)?;
            let lineage = args.required("lineage")?;
            let relation = if args.flag("contradicts") {
                EvidenceRelation::Contradicts
            } else {
                EvidenceRelation::Supports
            };
            let source = args.option("source").map(str::to_owned);
            args.validate_used()?;
            let p = args.positionals();
            let evidence = EvidenceContent {
                claim_id: parse_id(&p[2])?,
                kind: parse_enum::<EvidenceKind>(&p[3], "EvidenceKind", "invalid_arguments")?,
                reference: Some(p[4].clone()),
                lineage_id: Some(lineage),
                relation,
                source_id: source.as_deref().map(parse_id).transpose()?,
                source_hash: None,
            };
            memory.create_evidence(&evidence, provenance, key)?
        }
        RecordKind::Retex => {
            args.require_count(2)?;
            let retex = RetexContent {
                event: Some(args.required("event")?),
                impact: Some(args.required("impact")?),
                next_action: Some(args.required("next")?),
            };
            args.validate_used()?;
            memory.create_retex(&retex, provenance, key)?
        }
        RecordKind::Source => {
            return Err(SermofurError::invalid_arguments("Unknown kind."));
        }
    };
    to_json(&record)
}

#[cfg(test)]
mod tests {
    use super::{parse_enum, parse_id, writes};
    use crate::args::CommandArguments;
    use sermofur_core::records::ScopeKind;

    fn parsed(values: &[&str]) -> CommandArguments {
        let arguments: Vec<String> = values.iter().map(|v| (*v).to_owned()).collect();
        CommandArguments::parse(&arguments).expect("valid")
    }

    /// `CommandRunner.Writes`: the store opens read-write only for these.
    #[test]
    fn only_writing_commands_open_the_store_for_writing() {
        for line in [
            &["export"][..],
            &["claim", "add", "x"],
            &["claim", "invalidate", "x"],
            &["scope", "add"],
            &["source", "reindex"],
            &["index", "rebuild"],
            &["migrate"],
        ] {
            assert!(writes(&parsed(line)), "{line:?}");
        }
        for line in [
            &["status"][..],
            &["claim", "list"],
            &["claim", "show", "x"],
            &["scope", "current"],
            &["recall", "add"],
            &["source", "list"],
            &["frob", "add"],
        ] {
            assert_eq!(writes(&parsed(line)), line == ["frob", "add"], "{line:?}");
        }
    }

    #[test]
    fn enumerations_ignore_case_dashes_and_underscores() {
        let kind = |v| parse_enum::<ScopeKind>(v, "ScopeKind", "invalid_scope");
        assert_eq!(kind("-Re_po-sitory_"), Ok(ScopeKind::Repository));
        let error = kind("workspace1").expect_err("not a name");
        assert_eq!(
            (error.code.as_str(), error.message.as_str()),
            ("invalid_scope", "Invalid ScopeKind value.")
        );
        assert!(kind("").is_err());
        assert!(parse_id("00000000-0000-0000-0000-000000000000").is_err());
    }
}
