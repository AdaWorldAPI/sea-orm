//! Ontology-driven entity code generation for SeaORM.
//!
//! This module is part of the AdaWorldAPI/sea-orm integration plan §7
//! ("Ontology-driven entity codegen"). It defines the data structures that
//! mirror the YAML ontology format and exposes entry-point functions that
//! will be called by the `sea-orm-cli --from-ontology` flag.
//!
//! # SCAFFOLD NOTICE
//!
//! This file **locks the public API surface** agreed upon in Sprint 3.
//! It is intentionally a scaffold: all function bodies are stubs that
//! panic with `unimplemented!("SO-3 stub — Sprint 3")`. The actual
//! implementation will be wired in a follow-up sprint once the orchestrator
//! has:
//!
//! 1. Added `mod from_ontology;` + `pub use from_ontology::*;` to `lib.rs`.
//! 2. Added a `yaml` / `serde_yaml` dependency to `sea-orm-codegen/Cargo.toml`.
//! 3. Registered the `--from-ontology` CLI flag in `sea-orm-cli/src/cli.rs`.
//!
//! See integration-plan.md §7 for the full YAML input format and expected
//! entity output examples.

use std::path::Path;

// ─── Error type ──────────────────────────────────────────────────────────────

/// Errors that can occur while parsing or generating from an ontology YAML.
///
/// Variants map directly to the three failure modes described in plan §7:
/// file I/O failures, YAML deserialization errors, and semantic validation
/// errors (e.g. an edge referencing a node that does not exist).
#[derive(Debug)]
pub enum OntologyError {
    /// An I/O error occurred reading the YAML file from disk.
    Io(std::io::Error),
    /// The YAML content could not be parsed or did not match the expected schema.
    Yaml(String),
    /// The ontology is structurally valid YAML but is semantically invalid
    /// (e.g. an edge references an undefined node, or a required field is absent).
    Validation(String),
}

impl std::fmt::Display for OntologyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OntologyError::Io(e) => write!(f, "IO error reading ontology file: {e}"),
            OntologyError::Yaml(msg) => write!(f, "YAML parse error: {msg}"),
            OntologyError::Validation(msg) => write!(f, "Ontology validation error: {msg}"),
        }
    }
}

impl std::error::Error for OntologyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OntologyError::Io(e) => Some(e),
            OntologyError::Yaml(_) | OntologyError::Validation(_) => None,
        }
    }
}

impl From<std::io::Error> for OntologyError {
    fn from(e: std::io::Error) -> Self {
        OntologyError::Io(e)
    }
}

// ─── YAML input structures ────────────────────────────────────────────────────

/// A single column definition inside a node (entity) declaration.
///
/// Maps to one entry in the `columns:` list of a YAML `nodes:` entry.
///
/// # YAML Example
/// ```yaml
/// - name: subject
///   type: String
///   nullable: false
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntologyColumn {
    /// The snake_case column name; becomes the SeaORM `Column` variant.
    pub name: String,
    /// The Rust type string (e.g. `"String"`, `"i32"`, `"Uuid"`).
    pub column_type: String,
    /// Whether the column is nullable (`Option<T>` in the generated model).
    pub nullable: bool,
}

/// A node declaration — maps to one SeaORM entity (table).
///
/// Maps to one entry in the top-level `nodes:` list of the ontology YAML.
///
/// # YAML Example
/// ```yaml
/// - name: Ticket
///   pk: id
///   pk_type: Uuid
///   actor: true
///   columns:
///     - { name: subject, type: String, nullable: false }
///     - { name: created_at, type: DateTime, nullable: false }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntologyNode {
    /// The PascalCase entity name; becomes the module name (snake_case) and
    /// the `DeriveEntityModel` struct name.
    pub name: String,
    /// The primary-key column name (e.g. `"id"`).
    pub pk: String,
    /// The Rust type of the primary key (e.g. `"Uuid"`, `"i32"`).
    pub pk_type: String,
    /// When `true`, this node represents an actor / agent in the domain
    /// (semantic annotation only; no structural difference in the generated
    /// entity, but future tooling may emit additional traits).
    pub actor: bool,
    /// The non-PK columns of this node.
    pub columns: Vec<OntologyColumn>,
}

/// An optional property carried on an edge (junction-table column).
///
/// Maps to one entry in the `properties:` list of a YAML `edges:` entry.
///
/// # YAML Example
/// ```yaml
/// - name: created_at
///   type: DateTime
///   nullable: false
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntologyEdgeProperty {
    /// The snake_case property (column) name.
    pub name: String,
    /// The Rust type string.
    pub property_type: String,
    /// Whether the property is nullable.
    pub nullable: bool,
}

/// An edge declaration — maps to a SeaORM `Relation` between two entities,
/// and optionally a junction-table entity when `properties` are present.
///
/// Maps to one entry in the top-level `edges:` list of the ontology YAML.
///
/// # YAML Example
/// ```yaml
/// - name: TicketMsg
///   src: Ticket
///   dst: TicketStatus
///   properties:
///     - { name: sent_at, type: DateTime, nullable: false }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntologyEdge {
    /// The PascalCase edge name; used as the junction-table entity name
    /// when the edge carries properties.
    pub name: String,
    /// The source node name (must match an entry in `OntologyInput::nodes`).
    pub src: String,
    /// The destination node name (must match an entry in `OntologyInput::nodes`).
    pub dst: String,
    /// Additional columns on the junction table. If empty, the edge generates
    /// only a `has_many` / `belongs_to` relation without a junction entity.
    pub properties: Vec<OntologyEdgeProperty>,
}

/// The top-level parsed representation of an ontology YAML file.
///
/// Produced by [`parse_yaml`] and consumed by [`generate_entities`].
///
/// # Minimal YAML
/// ```yaml
/// nodes:
///   - name: Ticket
///     pk: id
///     pk_type: Uuid
///     actor: false
///     columns: []
/// edges: []
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntologyInput {
    /// All entity/node declarations in the ontology.
    pub nodes: Vec<OntologyNode>,
    /// All relationship/edge declarations in the ontology.
    pub edges: Vec<OntologyEdge>,
}

// ─── Output structures ────────────────────────────────────────────────────────

/// A single generated SeaORM entity ready to be written to disk.
///
/// Produced by [`generate_entities`].  The caller (the CLI `run` function)
/// is responsible for writing `source` to `<output_dir>/<module_name>.rs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedEntity {
    /// The snake_case module name (e.g. `"ticket"`, `"ticket_status"`).
    /// This becomes the output filename: `{module_name}.rs`.
    pub module_name: String,
    /// The complete Rust source text for the entity file.
    pub source: String,
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Parse an ontology YAML file from `path` into an [`OntologyInput`].
///
/// # Errors
///
/// Returns [`OntologyError::Io`] if the file cannot be read,
/// [`OntologyError::Yaml`] if YAML deserialization fails, and
/// [`OntologyError::Validation`] if the parsed structure fails semantic checks
/// (e.g. edge references a node not present in `nodes:`).
///
/// # Sprint 3 stub
///
/// The body is unimplemented. Integration is blocked on:
/// - Adding `serde_yaml` (or `serde-yaml`) to `sea-orm-codegen/Cargo.toml`.
/// - Deriving `serde::Deserialize` on the `Ontology*` structs.
/// - Implementing the validation pass.
pub fn parse_yaml(_path: &Path) -> Result<OntologyInput, OntologyError> {
    unimplemented!("SO-3 stub — Sprint 3")
}

/// Generate SeaORM entity source files from a parsed [`OntologyInput`].
///
/// Each [`OntologyNode`] produces one [`GeneratedEntity`].  Each
/// [`OntologyEdge`] that carries `properties` additionally produces a
/// junction-table entity.
///
/// The generated source follows the SeaORM 2.0 `#[sea_orm::model]` format
/// documented in `CLAUDE.md` (relations defined directly on `Model`, no
/// separate `Relation` enum).
///
/// # Sprint 3 stub
///
/// The body is unimplemented. The full generator will be wired in Sprint 4
/// once the CLI plumbing and YAML parsing are in place.
pub fn generate_entities(_input: &OntologyInput) -> Vec<GeneratedEntity> {
    unimplemented!("SO-3 stub — Sprint 3")
}
