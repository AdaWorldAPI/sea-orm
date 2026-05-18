//! # `#[derive(SeaOrmActor)]` — integration-plan §5
//!
//! Procedural macro that implements [`sea_orm_ractor::EntityActor`] for the entity that
//! lives in `super::Entity`, relative to the `Model` struct being annotated.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use sea_orm::entity::prelude::*;
//!
//! mod ticket {
//!     use sea_orm::entity::prelude::*;
//!
//!     #[sea_orm::model]
//!     #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, SeaOrmActor)]
//!     #[sea_orm(table_name = "ticket")]
//!     #[actor(msg = "TicketMsg")]
//!     pub struct Model {
//!         #[sea_orm(primary_key)]
//!         pub id: i64,
//!         pub status: String,
//!     }
//!
//!     #[derive(Debug)]
//!     pub enum TicketMsg { Assign(i64), Resolve, Escalate }
//!
//!     impl ActiveModelBehavior for ActiveModel {}
//! }
//!
//! // Consumer idiom (plan §5):
//! // ticket::Entity::actor(4711).send_message(ticket::TicketMsg::Escalate)?;
//! ```
//!
//! ## Generated code shape
//!
//! ```rust,ignore
//! impl ::sea_orm_ractor::entity_actor::EntityActor for super::Entity {
//!     type ActorMsg        = TicketMsg;
//!     type ActorPrimaryKey = i64;
//!
//!     fn actor(pk: i64) -> ::ractor::ActorRef<TicketMsg> {
//!         REGISTRY.get_or_spawn(pk)
//!     }
//! }
//!
//! static REGISTRY: ::std::sync::LazyLock<
//!     ::sea_orm_ractor::registry::EntityActorRegistry<super::Entity>
//! > = ::std::sync::LazyLock::new(|| {
//!     ::sea_orm_ractor::registry::EntityActorRegistry::new(|_pk| {
//!         unimplemented!("SeaOrmActor derive: spawn closure not yet implemented — Sprint 2")
//!     })
//! });
//! ```
//!
//! *plan §5 reference*: Glue #3 — `sea-orm-ractor` derive macro.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, Type};

/// Core expand function for the `SeaOrmActor` derive.
///
/// Called from `sea_orm_macros::lib::expand_derive_sea_orm_actor`.
pub fn expand_derive_sea_orm_actor(input: DeriveInput) -> syn::Result<TokenStream> {
    let span = input.ident.span();

    // ── 1. Validate: only applicable on `Model` struct ──────────────────────
    if input.ident != "Model" {
        return Err(syn::Error::new(
            span,
            "#[derive(SeaOrmActor)] must be placed on a struct named `Model`",
        ));
    }

    // ── 2. Parse the #[actor(msg = "...")] outer attribute ───────────────────
    let msg_type: Type = parse_actor_msg_attr(&input, span)?;

    // ── 3. Find the primary key field type from #[sea_orm(primary_key)] ──────
    let pk_type: Type = find_primary_key_type(&input, span)?;

    // ── 4. Emit the impl + static registry ───────────────────────────────────
    Ok(emit_impl(msg_type, pk_type))
}

// ── Attribute parsing helpers ────────────────────────────────────────────────

/// Extract the message type from `#[actor(msg = "MsgType")]`.
///
/// Returns an error with a helpful message if the attribute is absent or malformed.
fn parse_actor_msg_attr(input: &DeriveInput, span: Span) -> syn::Result<Type> {
    let mut found: Option<Type> = None;
    let mut attr_present = false;

    for attr in &input.attrs {
        if attr.path().is_ident("actor") {
            attr_present = true;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("msg") {
                    let litstr: LitStr = meta.value()?.parse()?;
                    let ty: Type = syn::parse_str(&litstr.value()).map_err(|e| {
                        syn::Error::new(
                            litstr.span(),
                            format!("#[actor(msg = \"...\")] value is not a valid type: {e}"),
                        )
                    })?;
                    found = Some(ty);
                } else {
                    // consume unknown keys without error (forward-compat)
                    let _ = meta.value().and_then(|v| v.parse::<syn::Expr>());
                }
                Ok(())
            })?;
        }
    }

    if !attr_present || found.is_none() {
        return Err(syn::Error::new(
            span,
            "SeaOrmActor requires #[actor(msg = \"YourMsgType\")] attribute on the Model struct",
        ));
    }

    Ok(found.unwrap())
}

/// Find the primary-key field type by scanning for `#[sea_orm(primary_key)]`.
///
/// Constraints:
/// - Exactly one primary-key field → supported (`i32`, `i64`, `u32`, `u64`, `String`, …).
/// - Zero fields with `primary_key` → error.
/// - More than one field with `primary_key` → `compile_error!` with TODO Sprint 2 note.
fn find_primary_key_type(input: &DeriveInput, span: Span) -> syn::Result<Type> {
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new(
                    span,
                    "#[derive(SeaOrmActor)] only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                span,
                "#[derive(SeaOrmActor)] can only be applied to structs",
            ));
        }
    };

    let mut pk_types: Vec<Type> = Vec::new();

    for field in fields {
        let mut is_pk = false;
        for attr in &field.attrs {
            if attr.path().is_ident("sea_orm") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("primary_key") {
                        is_pk = true;
                    } else {
                        // consume value so the parser doesn't get stuck
                        let _ = meta.value().and_then(|v| v.parse::<syn::Expr>());
                    }
                    Ok(())
                })?;
            }
        }
        if is_pk {
            pk_types.push(field.ty.clone());
        }
    }

    match pk_types.len() {
        0 => Err(syn::Error::new(
            span,
            "#[derive(SeaOrmActor)]: no field marked #[sea_orm(primary_key)] found on Model",
        )),
        1 => Ok(pk_types.remove(0)),
        _ => {
            // Composite PK: emit compile_error! as per spec
            Err(syn::Error::new(
                span,
                "#[derive(SeaOrmActor)]: composite primary keys are not yet supported \
                 (Sprint 2 TODO). Use a single-column primary key (i32 / i64 / u32 / u64 / String).",
            ))
        }
    }
}

// ── Code generation ──────────────────────────────────────────────────────────

/// Emit the `EntityActor` impl and the `REGISTRY` static.
///
/// All external crate paths are fully qualified so this macro works without
/// importing anything in the consumer's module.  `sea_orm_ractor` and `ractor`
/// are NOT compile-time dependencies of `sea-orm-macros`; the emitted paths
/// are resolved in the *consumer* crate's compilation unit.
///
/// Generated shape (plan §5):
/// ```text
/// impl ::sea_orm_ractor::entity_actor::EntityActor for super::Entity {
///     type ActorMsg        = <msg_type>;
///     type ActorPrimaryKey = <pk_type>;
///     fn actor(pk: <pk_type>) -> ::ractor::ActorRef<<msg_type>> {
///         REGISTRY.get_or_spawn(pk)
///     }
/// }
///
/// static REGISTRY: ::std::sync::LazyLock<
///     ::sea_orm_ractor::registry::EntityActorRegistry<super::Entity>
/// > = ::std::sync::LazyLock::new(|| {
///     ::sea_orm_ractor::registry::EntityActorRegistry::new(|_pk| {
///         unimplemented!("SeaOrmActor derive: spawn closure — Sprint 2")
///     })
/// });
/// ```
fn emit_impl(msg_type: Type, pk_type: Type) -> TokenStream {
    quote! {
        #[automatically_derived]
        impl ::sea_orm_ractor::entity_actor::EntityActor for super::Entity {
            /// The message enum accepted by this entity's actor.
            /// Generated by `#[derive(SeaOrmActor)]` — integration-plan §5.
            type ActorMsg = #msg_type;

            /// The concrete primary-key value type for this entity.
            /// Generated by `#[derive(SeaOrmActor)]` — integration-plan §5.
            type ActorPrimaryKey = #pk_type;

            /// Resolve (or lazily spawn) the actor for `pk`.
            /// Delegates to the per-entity `REGISTRY` static.
            fn actor(pk: #pk_type) -> ::ractor::ActorRef<#msg_type> {
                REGISTRY.get_or_spawn(pk)
            }
        }

        /// Per-entity `ActorRef` registry, keyed on `ActorPrimaryKey`.
        ///
        /// Initialised on first access via `std::sync::LazyLock` (stable since Rust 1.85).
        /// The spawn closure body is a stub (`unimplemented!`) until Sprint 2 wires the
        /// async `ractor::Actor::spawn` call.
        ///
        /// *integration-plan §5 reference*
        #[doc(hidden)]
        static REGISTRY: ::std::sync::LazyLock<
            ::sea_orm_ractor::registry::EntityActorRegistry<super::Entity>
        > = ::std::sync::LazyLock::new(|| {
            ::sea_orm_ractor::registry::EntityActorRegistry::new(|_pk| {
                unimplemented!(
                    "SeaOrmActor derive: actor spawn closure is not yet implemented — \
                     wire ractor::Actor::spawn in Sprint 2 (integration-plan §5)"
                )
            })
        });
    }
}
