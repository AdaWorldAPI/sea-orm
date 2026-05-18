//! Ontology-driven entity code generation for SeaORM.
//!
//! This module is part of the AdaWorldAPI/sea-orm integration plan §7
//! ("Ontology-driven entity codegen"). It defines the data structures that
//! mirror the YAML ontology format and exposes entry-point functions that
//! will be called by the `sea-orm-cli --from-ontology` flag.
//!
//! See integration-plan.md §7 for the full YAML input format and expected
//! entity output examples.

use std::{collections::HashMap, path::Path};

use serde::Deserialize;

// ─── Error type ──────────────────────────────────────────────────────────────

/// Errors that can occur while parsing or generating from an ontology YAML.
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

// ─── YAML deserialization helpers ─────────────────────────────────────────────

/// Inline column specification in the plan §7 YAML format.
///
/// Supports both the short form (`id: UInt64`) and the long form
/// (`status: { type: String, enum: [Open, ...], unique: true }`).
///
/// Short form is parsed by [`NodeShapeRaw`]'s custom deserializer before this
/// struct is instantiated.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ColumnSpec {
    /// The YAML type string (`UInt64`, `Int32`, `String`, `Bool`, …).
    #[serde(rename = "type")]
    pub col_type: String,
    /// If set, emits a `DeriveActiveEnum` companion enum with these variants.
    #[serde(default)]
    pub enum_values: Vec<String>,
    /// When `true`, emits `#[sea_orm(unique)]` on the generated field.
    #[serde(default)]
    pub unique: bool,
    /// Foreign-key target node name (e.g. `User`). Generates a `belongs_to` relation.
    #[serde(default)]
    pub fk: Option<String>,
}

/// Raw serde representation of a single node in the plan §7 map-based YAML.
///
/// We parse the `columns:` map ourselves so that short forms (`id: UInt64`)
/// and long forms (`status: { type: String, enum: [...] }`) both deserialize
/// into [`ColumnSpec`] values.
#[derive(Clone, Debug, Deserialize)]
struct NodeShapeRaw {
    pk: String,
    #[serde(default)]
    actor: bool,
    #[serde(default)]
    columns: HashMap<String, serde_yaml::Value>,
    /// Ordered column names; populated by re-parsing insertion order.
    #[serde(skip)]
    _column_order: Vec<String>,
}

/// Raw serde representation of a single edge in the YAML.
#[derive(Clone, Debug, Deserialize)]
struct EdgeShapeRaw {
    src: String,
    dst: String,
}

/// Raw top-level serde representation of the YAML file.
///
/// `nodes` is a mapping from node-name → [`NodeShapeRaw`].
/// `edges` is a mapping from edge-name → [`EdgeShapeRaw`].
#[derive(Clone, Debug, Deserialize)]
struct OntologyInputRaw {
    #[serde(default)]
    nodes: HashMap<String, NodeShapeRaw>,
    #[serde(default)]
    edges: HashMap<String, EdgeShapeRaw>,
}

// ─── Public input structures ──────────────────────────────────────────────────

/// A single column definition inside a node (entity) declaration.
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntologyNode {
    /// The PascalCase entity name.
    pub name: String,
    /// The primary-key column name (e.g. `"id"`).
    pub pk: String,
    /// The Rust type of the primary key (e.g. `"i64"`, `"i32"`).
    pub pk_type: String,
    /// When `true`, also emits `#[derive(SeaOrmActor)]` and a `<Name>Msg` enum.
    pub actor: bool,
    /// The non-PK columns of this node (in source order).
    pub columns: Vec<OntologyColumn>,
    /// Full column specs (includes pk), keyed by column name.
    pub column_specs: HashMap<String, ColumnSpec>,
    /// Column names in their original source order.
    pub column_order: Vec<String>,
}

/// An edge declaration — maps to a SeaORM `has_many` relation on the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntologyEdge {
    /// The edge name (used to identify it; not emitted directly).
    pub name: String,
    /// Source node name.
    pub src: String,
    /// Destination node name.
    pub dst: String,
}

/// The top-level parsed representation of an ontology YAML file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OntologyInput {
    /// All entity/node declarations in stable insertion order.
    pub nodes: Vec<OntologyNode>,
    /// All relationship/edge declarations.
    pub edges: Vec<OntologyEdge>,
}

// ─── Output structures ────────────────────────────────────────────────────────

/// A single generated SeaORM entity ready to be written to disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedEntity {
    /// The snake_case module name (e.g. `"ticket"`, `"ticket_status"`).
    pub module_name: String,
    /// The complete Rust source text for the entity file.
    pub source: String,
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Parse an ontology YAML file from `path` into an [`OntologyInput`].
///
/// Accepts the plan §7 map-based YAML format:
/// ```yaml
/// nodes:
///   Ticket:
///     pk: id
///     actor: true
///     columns:
///       id: UInt64
///       status: { type: String, enum: [Open, Closed] }
/// edges:
///   TicketToUser:
///     src: Ticket
///     dst: User
/// ```
///
/// # Errors
/// - [`OntologyError::Io`] — file cannot be read
/// - [`OntologyError::Yaml`] — YAML parse failure
/// - [`OntologyError::Validation`] — unknown column type or bad references
pub fn parse_yaml(path: &Path) -> Result<OntologyInput, OntologyError> {
    // 1. Read
    let text = std::fs::read_to_string(path)?;

    // 2. Parse raw using serde_yaml.
    //    We parse to serde_yaml::Value first so we can handle the ordering of
    //    the `columns:` map, which serde's HashMap does not preserve.
    let raw_value: serde_yaml::Value =
        serde_yaml::from_str(&text).map_err(|e| OntologyError::Yaml(e.to_string()))?;

    parse_yaml_value(&raw_value)
}

/// Parse an [`OntologyInput`] from an already-loaded [`serde_yaml::Value`].
///
/// Used internally and by tests (which can build the value from a string
/// without a file).
fn parse_yaml_value(value: &serde_yaml::Value) -> Result<OntologyInput, OntologyError> {
    // Extract top-level `nodes` mapping (required).
    let nodes_value = value
        .get("nodes")
        .ok_or_else(|| OntologyError::Validation("missing top-level `nodes:` key".into()))?;

    let nodes_map = nodes_value
        .as_mapping()
        .ok_or_else(|| OntologyError::Validation("`nodes:` must be a mapping".into()))?;

    let mut nodes: Vec<OntologyNode> = Vec::new();

    for (name_val, node_val) in nodes_map {
        let node_name = name_val
            .as_str()
            .ok_or_else(|| OntologyError::Validation("node name must be a string".into()))?
            .to_owned();

        let pk = node_val
            .get("pk")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                OntologyError::Validation(format!("node `{node_name}` missing `pk:` field"))
            })?
            .to_owned();

        let actor = node_val
            .get("actor")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Parse columns mapping, preserving order.
        let columns_value = node_val.get("columns");
        let mut column_order: Vec<String> = Vec::new();
        let mut column_specs: HashMap<String, ColumnSpec> = HashMap::new();

        if let Some(cols) = columns_value {
            let cols_map = cols.as_mapping().ok_or_else(|| {
                OntologyError::Validation(format!(
                    "node `{node_name}`: `columns:` must be a mapping"
                ))
            })?;

            for (col_name_val, col_spec_val) in cols_map {
                let col_name = col_name_val
                    .as_str()
                    .ok_or_else(|| {
                        OntologyError::Validation(format!(
                            "node `{node_name}`: column name must be a string"
                        ))
                    })?
                    .to_owned();

                let spec = parse_column_spec(&node_name, &col_name, col_spec_val)?;
                column_order.push(col_name.clone());
                column_specs.insert(col_name, spec);
            }
        }

        // Determine pk_type from the pk column spec.
        let pk_rust_type = if let Some(spec) = column_specs.get(&pk) {
            map_yaml_type_to_rust(&spec.col_type).map_err(|e| OntologyError::Validation(e))?
        } else {
            // pk column not listed in columns: (allowed — default to i64)
            "i64".to_owned()
        };

        // Build OntologyColumn list for non-pk columns.
        let columns: Vec<OntologyColumn> = column_order
            .iter()
            .filter(|cn| **cn != pk)
            .map(|cn| {
                let spec = &column_specs[cn];
                let rust_type = map_yaml_type_to_rust(&spec.col_type)
                    .unwrap_or_else(|_| spec.col_type.clone());
                OntologyColumn {
                    name: cn.clone(),
                    column_type: rust_type,
                    nullable: false, // plan §7 format: nullable is implicit
                }
            })
            .collect();

        nodes.push(OntologyNode {
            name: node_name,
            pk,
            pk_type: pk_rust_type,
            actor,
            columns,
            column_specs,
            column_order,
        });
    }

    // Extract optional `edges` mapping.
    let mut edges: Vec<OntologyEdge> = Vec::new();

    if let Some(edges_value) = value.get("edges") {
        let edges_map = edges_value.as_mapping().ok_or_else(|| {
            OntologyError::Validation("`edges:` must be a mapping".into())
        })?;

        for (edge_name_val, edge_val) in edges_map {
            let edge_name = edge_name_val
                .as_str()
                .ok_or_else(|| OntologyError::Validation("edge name must be a string".into()))?
                .to_owned();

            let src = edge_val
                .get("src")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    OntologyError::Validation(format!("edge `{edge_name}` missing `src:` field"))
                })?
                .to_owned();

            let dst = edge_val
                .get("dst")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    OntologyError::Validation(format!("edge `{edge_name}` missing `dst:` field"))
                })?
                .to_owned();

            edges.push(OntologyEdge {
                name: edge_name,
                src,
                dst,
            });
        }
    }

    // Validate: edge src/dst must reference known nodes.
    let node_names: std::collections::HashSet<&str> =
        nodes.iter().map(|n| n.name.as_str()).collect();

    for edge in &edges {
        if !node_names.contains(edge.src.as_str()) {
            return Err(OntologyError::Validation(format!(
                "edge `{}` references unknown src node `{}`",
                edge.name, edge.src
            )));
        }
        if !node_names.contains(edge.dst.as_str()) {
            return Err(OntologyError::Validation(format!(
                "edge `{}` references unknown dst node `{}`",
                edge.name, edge.dst
            )));
        }
    }

    Ok(OntologyInput { nodes, edges })
}

/// Parse a single column spec value, handling both short (`UInt64`) and long
/// (`{ type: String, enum: [...], unique: true }`) forms.
fn parse_column_spec(
    node_name: &str,
    col_name: &str,
    value: &serde_yaml::Value,
) -> Result<ColumnSpec, OntologyError> {
    match value {
        // Short form: `id: UInt64`
        serde_yaml::Value::String(type_str) => Ok(ColumnSpec {
            col_type: type_str.clone(),
            enum_values: Vec::new(),
            unique: false,
            fk: None,
        }),
        // Long form: `status: { type: String, enum: [...], unique: true }`
        serde_yaml::Value::Mapping(_) => {
            let col_type = value
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    OntologyError::Validation(format!(
                        "node `{node_name}` column `{col_name}`: missing `type:` field"
                    ))
                })?
                .to_owned();

            let enum_values = if let Some(ev) = value.get("enum") {
                ev.as_sequence()
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            let unique = value
                .get("unique")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let fk = value
                .get("fk")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned());

            Ok(ColumnSpec {
                col_type,
                enum_values,
                unique,
                fk,
            })
        }
        _ => Err(OntologyError::Validation(format!(
            "node `{node_name}` column `{col_name}`: unexpected value type"
        ))),
    }
}

/// Map a YAML type string to a Rust type string.
///
/// Returns `Err` with an `OntologyError::Validation`-ready message on unknown types.
fn map_yaml_type_to_rust(yaml_type: &str) -> Result<String, String> {
    let rust = match yaml_type {
        "UInt64" | "u64" => "i64", // SeaORM uses i64 for unsigned 64-bit
        "UInt32" | "u32" => "i32",
        "Int64" | "i64" => "i64",
        "Int32" | "i32" => "i32",
        "String" => "String",
        "Bool" | "bool" => "bool",
        "Date" => "chrono::NaiveDate",
        "DateTime" => "chrono::DateTime<chrono::Utc>",
        "Uuid" => "Uuid",
        "Text" => "String", // Text is stored as String in Rust
        other => return Err(format!("unsupported column type: `{other}`")),
    };
    Ok(rust.to_owned())
}

/// Convert a PascalCase or camelCase name to snake_case.
///
/// Examples: `Ticket` → `ticket`, `TicketStatus` → `ticket_status`.
fn pascal_to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

// ─── Code generation ──────────────────────────────────────────────────────────

/// Generate SeaORM entity source files from a parsed [`OntologyInput`].
///
/// Each [`OntologyNode`] produces one [`GeneratedEntity`].
/// For each edge, a `has_many` relation is appended to the source entity.
/// Nodes with `actor: true` get a `#[derive(SeaOrmActor)]` + a `<Name>Msg` enum.
pub fn generate_entities(input: &OntologyInput) -> Vec<GeneratedEntity> {
    // Index edges by src node so we can append `has_many` relations.
    let mut src_edges: HashMap<&str, Vec<&OntologyEdge>> = HashMap::new();
    for edge in &input.edges {
        src_edges.entry(edge.src.as_str()).or_default().push(edge);
    }

    input
        .nodes
        .iter()
        .map(|node| generate_node_entity(node, &src_edges))
        .collect()
}

/// Generate a single entity source string for one [`OntologyNode`].
fn generate_node_entity(
    node: &OntologyNode,
    src_edges: &HashMap<&str, Vec<&OntologyEdge>>,
) -> GeneratedEntity {
    let module_name = pascal_to_snake(&node.name);
    let table_name = module_name.clone();

    let mut src = String::new();

    // File header comment.
    src.push_str(&format!(
        "//! Generated by sea-orm-cli --from-ontology\n//! Source node: \"{}\"\n\n",
        node.name
    ));

    // Imports.
    src.push_str("use sea_orm::entity::prelude::*;\n");
    if node.actor {
        src.push_str("use sea_orm_ractor::SeaOrmActor;\n");
    }
    src.push('\n');

    // Derive list.
    let mut derives = vec![
        "Clone",
        "Debug",
        "PartialEq",
        "Eq",
        "DeriveEntityModel",
    ];
    if node.actor {
        derives.push("SeaOrmActor");
    }
    let derives_str = derives.join(", ");

    src.push_str("#[sea_orm::model]\n");
    src.push_str(&format!("#[derive({derives_str})]\n"));
    src.push_str(&format!("#[sea_orm(table_name = \"{table_name}\")]\n"));
    if node.actor {
        src.push_str(&format!("#[actor(msg = \"{}Msg\")]\n", node.name));
    }
    src.push_str("pub struct Model {\n");

    // Primary key field.
    let pk_type = &node.pk_type;
    let pk_auto_increment = pk_type == "i32";
    if pk_auto_increment {
        src.push_str("    #[sea_orm(primary_key)]\n");
    } else {
        src.push_str("    #[sea_orm(primary_key, auto_increment = false)]\n");
    }
    src.push_str(&format!("    pub {}: {pk_type},\n", node.pk));

    // Non-PK scalar columns.
    for col in &node.columns {
        let spec = node.column_specs.get(&col.name);

        // Check if this column has an enum — if so, use the enum type name.
        let field_type = if let Some(s) = spec {
            if !s.enum_values.is_empty() {
                // Enum type name: PascalCase(node.name) + PascalCase(col.name)
                format!(
                    "{}{}",
                    node.name,
                    col.name
                        .split('_')
                        .map(|w| {
                            let mut c = w.chars();
                            match c.next() {
                                None => String::new(),
                                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                            }
                        })
                        .collect::<String>()
                )
            } else if s.fk.is_some() {
                // FK columns: use raw Rust type (they're scalar values in the Model)
                col.column_type.clone()
            } else {
                col.column_type.clone()
            }
        } else {
            col.column_type.clone()
        };

        let field_type = if col.nullable {
            format!("Option<{field_type}>")
        } else {
            field_type
        };

        // SeaORM annotations for this column.
        let mut attrs: Vec<String> = Vec::new();
        if let Some(s) = spec {
            if s.unique {
                attrs.push("unique".to_owned());
            }
            // Text type needs column_type annotation for MySQL safety.
            if s.col_type == "Text" {
                attrs.push("column_type = \"Text\"".to_owned());
            }
        }

        if !attrs.is_empty() {
            src.push_str(&format!("    #[sea_orm({})]\n", attrs.join(", ")));
        }
        src.push_str(&format!("    pub {}: {field_type},\n", col.name));
    }

    // `has_many` relations for edges originating from this node.
    if let Some(edges) = src_edges.get(node.name.as_str()) {
        for edge in edges.iter() {
            let dst_snake = pascal_to_snake(&edge.dst);
            src.push_str(&format!(
                "    #[sea_orm(has_many)]\n    pub {dst_snake}s: HasMany<super::{dst_snake}::Entity>,\n"
            ));
        }
    }

    src.push_str("}\n\n");

    // Enum companions for columns with `enum: [...]`.
    for col in &node.columns {
        if let Some(spec) = node.column_specs.get(&col.name) {
            if !spec.enum_values.is_empty() {
                let enum_name = format!(
                    "{}{}",
                    node.name,
                    col.name
                        .split('_')
                        .map(|w| {
                            let mut c = w.chars();
                            match c.next() {
                                None => String::new(),
                                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                            }
                        })
                        .collect::<String>()
                );

                src.push_str("#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]\n");
                src.push_str("#[sea_orm(rs_type = \"String\", db_type = \"String(StringLen::N(16))\")]\n");
                src.push_str(&format!("pub enum {enum_name} {{\n"));
                for variant in &spec.enum_values {
                    src.push_str(&format!(
                        "    #[sea_orm(string_value = \"{variant}\")] {variant},\n"
                    ));
                }
                src.push_str("}\n\n");
            }
        }
    }

    // Actor message enum.
    if node.actor {
        let msg_enum = format!("{}Msg", node.name);
        src.push_str("#[derive(Debug)]\n");
        src.push_str(&format!("pub enum {msg_enum} {{\n"));
        src.push_str("    Assign(i64),\n");
        src.push_str("    Resolve,\n");
        src.push_str("    Escalate,\n");
        src.push_str("}\n\n");
    }

    // ActiveModelBehavior impl.
    src.push_str("impl ActiveModelBehavior for ActiveModel {}\n");

    GeneratedEntity {
        module_name,
        source: src,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    // ── Minimal YAML fixture used across tests ──────────────────────────────

    const SIMPLE_YAML: &str = r#"
nodes:
  Person:
    pk: id
    actor: false
    columns:
      id: UInt64
      name: String
      age: Int32
edges: {}
"#;

    const ACTOR_YAML: &str = r#"
nodes:
  Ticket:
    pk: id
    actor: true
    columns:
      id: UInt64
      status: { type: String, enum: [Open, Assigned, Resolved, Escalated] }
      priority: Int32
edges: {}
"#;

    const EDGE_YAML: &str = r#"
nodes:
  Ticket:
    pk: id
    actor: false
    columns:
      id: UInt64
      title: String
  User:
    pk: id
    actor: false
    columns:
      id: UInt64
      username: String
edges:
  TicketToUser:
    src: Ticket
    dst: User
"#;

    // ── Helper: parse YAML from a &str without touching the filesystem ──────

    fn parse_str(yaml: &str) -> Result<OntologyInput, OntologyError> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(yaml).map_err(|e| OntologyError::Yaml(e.to_string()))?;
        parse_yaml_value(&value)
    }

    // ── Test 1: parse_yaml round-trip ───────────────────────────────────────

    /// Write a YAML fixture to a temp file, parse it, and assert the parsed
    /// structure matches what we constructed by hand.
    #[test]
    fn parse_yaml_round_trip() {
        let yaml = SIMPLE_YAML;

        // Write to a temp file.
        let mut tmp = tempfile::NamedTempFile::new().expect("tempfile");
        tmp.write_all(yaml.as_bytes()).unwrap();
        let path = tmp.path().to_path_buf();

        let input = parse_yaml(&path).expect("parse_yaml should succeed");

        assert_eq!(input.nodes.len(), 1);
        let person = &input.nodes[0];
        assert_eq!(person.name, "Person");
        assert_eq!(person.pk, "id");
        assert_eq!(person.pk_type, "i64"); // UInt64 → i64
        assert!(!person.actor);
        // `id` is the PK so only `name` and `age` are in `columns`
        assert_eq!(person.columns.len(), 2);
        assert_eq!(person.columns[0].name, "name");
        assert_eq!(person.columns[0].column_type, "String");
        assert_eq!(person.columns[1].name, "age");
        assert_eq!(person.columns[1].column_type, "i32");
        assert!(input.edges.is_empty());
    }

    // ── Test 2: generate_simple_node ────────────────────────────────────────

    /// Given a Person node with `id, name, age` columns, the generated source
    /// must contain the expected SeaORM 2.0 attributes and field declarations.
    #[test]
    fn generate_simple_node() {
        let input = parse_str(SIMPLE_YAML).expect("parse failed");
        let entities = generate_entities(&input);

        assert_eq!(entities.len(), 1);
        let entity = &entities[0];
        assert_eq!(entity.module_name, "person");

        let src = &entity.source;
        assert!(
            src.contains("#[sea_orm(table_name = \"person\")]"),
            "missing table_name attr; got:\n{src}"
        );
        assert!(
            src.contains("pub id: i64"),
            "missing `pub id: i64`; got:\n{src}"
        );
        assert!(
            src.contains("pub name: String"),
            "missing `pub name: String`; got:\n{src}"
        );
        assert!(
            src.contains("pub age: i32"),
            "missing `pub age: i32`; got:\n{src}"
        );
        assert!(
            src.contains("impl ActiveModelBehavior for ActiveModel {}"),
            "missing ActiveModelBehavior impl; got:\n{src}"
        );
        // No actor artifacts.
        assert!(
            !src.contains("SeaOrmActor"),
            "unexpected SeaOrmActor in non-actor node; got:\n{src}"
        );
    }

    // ── Test 3: generate_with_actor_opts_in ─────────────────────────────────

    /// A node with `actor: true` must produce:
    /// - `#[derive(... SeaOrmActor)]`
    /// - `#[actor(msg = "TicketMsg")]`
    /// - A `pub enum TicketMsg { Assign(i64), Resolve, Escalate, }` block
    /// - A companion `TicketStatus` DeriveActiveEnum for the enum column
    #[test]
    fn generate_with_actor_opts_in() {
        let input = parse_str(ACTOR_YAML).expect("parse failed");
        let entities = generate_entities(&input);

        assert_eq!(entities.len(), 1);
        let entity = &entities[0];
        assert_eq!(entity.module_name, "ticket");

        let src = &entity.source;
        assert!(
            src.contains("SeaOrmActor"),
            "expected SeaOrmActor derive; got:\n{src}"
        );
        assert!(
            src.contains("#[actor(msg = \"TicketMsg\")]"),
            "expected actor(msg) attribute; got:\n{src}"
        );
        assert!(
            src.contains("pub enum TicketMsg"),
            "expected TicketMsg enum; got:\n{src}"
        );
        assert!(
            src.contains("Assign(i64)"),
            "expected Assign variant; got:\n{src}"
        );
        assert!(
            src.contains("Resolve"),
            "expected Resolve variant; got:\n{src}"
        );
        assert!(
            src.contains("Escalate"),
            "expected Escalate variant; got:\n{src}"
        );
        // Enum companion for `status` column.
        assert!(
            src.contains("pub enum TicketStatus"),
            "expected TicketStatus enum companion; got:\n{src}"
        );
        assert!(
            src.contains("DeriveActiveEnum"),
            "expected DeriveActiveEnum; got:\n{src}"
        );
    }

    // ── Bonus test: has_many edge generation ────────────────────────────────

    #[test]
    fn generate_has_many_edge() {
        let input = parse_str(EDGE_YAML).expect("parse failed");
        let entities = generate_entities(&input);

        // Find the Ticket entity.
        let ticket_entity = entities
            .iter()
            .find(|e| e.module_name == "ticket")
            .expect("ticket entity not found");

        let src = &ticket_entity.source;
        assert!(
            src.contains("HasMany<super::user::Entity>"),
            "expected has_many relation to user; got:\n{src}"
        );
        assert!(
            src.contains("#[sea_orm(has_many)]"),
            "expected has_many attribute; got:\n{src}"
        );
    }
}
