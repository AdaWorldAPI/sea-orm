//! # sea-orm-ractor — Glue #3: dispatch-tag binding for SeaORM entities
//!
//! This crate implements the actor dispatch mechanism described in integration-plan §5.
//! It binds each SeaORM `Entity::PrimaryKey` value to a live `ractor` process in the
//! global process registry, enabling consumer-side idioms such as:
//!
//! ```text
//! Ticket::Entity::actor(4711).send_message(TicketMsg::Assign { user_id: 99 }).unwrap();
//! ```
//!
//! ## Architecture (plan §5 summary)
//!
//! * [`entity_actor::EntityActor`] — supertrait of `sea_orm::EntityTrait` that adds the
//!   `ActorMsg` / `ActorPrimaryKey` associated types and the `actor(pk)` factory method.
//!   `ActorPrimaryKey` (not `PrimaryKey`) avoids ambiguity with the inherited `EntityTrait::PrimaryKey`
//!   column-enum type.
//! * [`registry::EntityActorRegistry`] — runtime map of `ActorPrimaryKey → ActorRef<ActorMsg>`,
//!   backed by `RwLock<HashMap<…>>`.  Spawns on first access (`get_or_spawn`).
//!
//! ## Derive macro
//!
//! The `#[derive(SeaOrmActor)]` proc-macro lives in `sea-orm-macros` and is **out of
//! scope for Sprint 1** (SO-1 stub).  It will generate the boilerplate `EntityActor`
//! impl and wire up `EntityActorRegistry` as a `once_cell::sync::Lazy` static.  See
//! plan §5 for the expected generated code shape.
//!
//! ## Crate status
//!
//! Sprint 1 — stub only.  Bodies that require full ractor/sea-orm wiring use
//! `unimplemented!("SO-1 stub — Sprint 1")`.

pub mod entity_actor;
pub mod registry;
