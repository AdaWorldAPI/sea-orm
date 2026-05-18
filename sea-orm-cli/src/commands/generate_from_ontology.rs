//! CLI command: `sea-orm-cli generate from-ontology`
//!
//! This module is part of the AdaWorldAPI/sea-orm integration plan §7
//! ("Ontology-driven entity codegen"). It implements the `--from-ontology`
//! sub-command that reads a YAML ontology file and writes SeaORM entity
//! source files to an output directory by delegating to
//! [`sea_orm_codegen::from_ontology`].
//!
//! # SCAFFOLD NOTICE
//!
//! This file **locks the public CLI command surface** agreed upon in Sprint 3.
//! The function body is a stub that panics with
//! `unimplemented!("SO-3 stub — Sprint 3")`. The actual implementation will
//! be wired in a follow-up sprint once the orchestrator has:
//!
//! 1. Added `mod generate_from_ontology;` to
//!    `sea-orm-cli/src/commands/mod.rs`.
//! 2. Added a `FromOntology` variant (with `yaml_path` + `output_dir` args)
//!    to `GenerateSubcommands` in `sea-orm-cli/src/cli.rs` and routed it here.
//! 3. Added `anyhow = "1"` to `sea-orm-cli/Cargo.toml` (or confirmed it is
//!    already transitively available), replacing the interim
//!    `Box<dyn std::error::Error + Send + Sync>` return type below with
//!    `anyhow::Result<()>`.
//! 4. Added `mod from_ontology;` + `pub use from_ontology::*;` to
//!    `sea-orm-codegen/src/lib.rs` so that `sea_orm_codegen::from_ontology`
//!    is accessible.
//! 5. Added `serde_yaml` + `serde` dependencies to
//!    `sea-orm-codegen/Cargo.toml`.
//!
//! See integration-plan.md §7 for the full YAML input format and expected
//! entity output examples.
//!
//! # Return type note
//!
//! The final signature (once `anyhow` is wired in) will be:
//! ```text
//! pub async fn run(yaml_path: PathBuf, output_dir: PathBuf) -> anyhow::Result<()>
//! ```
//! For now the interim scaffold uses `Box<dyn std::error::Error + Send + Sync>`
//! so that this file can be parse-checked without the `anyhow` crate present.

use std::path::PathBuf;

/// Run the `generate from-ontology` command.
///
/// Reads the ontology YAML at `yaml_path`, delegates to
/// `sea_orm_codegen::from_ontology::parse_yaml` and
/// `sea_orm_codegen::from_ontology::generate_entities`, then writes the
/// resulting entity source files into `output_dir`.
///
/// # Arguments
///
/// * `yaml_path` — Path to the ontology YAML input file (e.g.
///   `ontology.yaml`). Must be a readable file whose contents conform to the
///   schema described in `from_ontology_examples.md`.
/// * `output_dir` — Directory in which to write the generated `*.rs` entity
///   files. Will be created (including parents) if it does not already exist.
///
/// # Errors
///
/// Propagates any error returned by `parse_yaml`, `generate_entities`, or
/// the filesystem operations (directory creation, file write). In the final
/// implementation these will be `anyhow::Error` variants with contextual
/// messages.
///
/// # Sprint 3 stub
///
/// The body is `unimplemented!`. This function will be implemented in Sprint 4.
pub async fn run(_yaml_path: PathBuf, _output_dir: PathBuf) -> anyhow::Result<()> {
    // Sprint 4 implementation stub.
    //
    // When implementing, wire in:
    //   let input = sea_orm_codegen::from_ontology::parse_yaml(&_yaml_path)?;
    //   let entities = sea_orm_codegen::from_ontology::generate_entities(&input);
    //   std::fs::create_dir_all(&_output_dir)?;
    //   for entity in entities {
    //       let path = _output_dir.join(format!("{}.rs", entity.module_name));
    //       std::fs::write(&path, &entity.source)?;
    //       println!("Writing {}", path.display());
    //   }
    //   Ok(())
    unimplemented!("SO-3 stub — Sprint 3")
}
