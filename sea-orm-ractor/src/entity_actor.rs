//! # `EntityActor` trait — integration-plan §5
//!
//! Supertrait of [`sea_orm::EntityTrait`] that attaches actor-dispatch semantics to a
//! SeaORM entity.  By implementing this trait (or deriving it via the forthcoming
//! `#[derive(SeaOrmActor)]` macro in `sea-orm-macros`), an entity gains a stable
//! `actor(pk)` factory that resolves or spawns the corresponding `ractor` process.

use std::hash::Hash;

/// Extension of [`sea_orm::EntityTrait`] that binds an entity to a `ractor` actor.
///
/// # Associated types
///
/// * `ActorMsg` — the message enum the actor accepts (must implement [`ractor::Message`]).
/// * `ActorPrimaryKey` — the concrete primary-key *value* type (e.g. `i64`, `uuid::Uuid`).
///   Distinct from `sea_orm::EntityTrait::PrimaryKey`, which is an enum of *columns*.
///   Named `ActorPrimaryKey` to avoid the ambiguity with the inherited `EntityTrait::PrimaryKey`.
///   Must be `Eq + Hash + Clone + Send + Sync + 'static` so it can be used as a `HashMap`
///   key and sent across threads.
///
/// # Implementor note (plan §5)
///
/// Manual implementation is rarely needed.  The `#[derive(SeaOrmActor)]` proc-macro
/// (Sprint 2, `sea-orm-macros`) will emit the boilerplate automatically.  Implement
/// manually only when you need a custom message type or non-trivial spawn logic.
///
/// # Example (consumer side)
///
/// ```rust,ignore
/// // Resolves the actor for ticket 4711, spawning it if not yet alive.
/// Ticket::Entity::actor(4711).send_message(TicketMsg::Assign { user_id: 99 }).unwrap();
/// ```
pub trait EntityActor: sea_orm::EntityTrait {
    /// The message type accepted by this entity's actor.
    ///
    /// Must satisfy `ractor::Message` (i.e. `Send + 'static`) and `'static`.
    ///
    /// *plan §5 reference*: "each entity exposes one `ActorMsg` enum".
    type ActorMsg: ractor::Message + 'static;

    /// The concrete primary-key *value* type for this entity.
    ///
    /// Named `ActorPrimaryKey` (not `PrimaryKey`) to avoid shadowing / ambiguity with
    /// `sea_orm::EntityTrait::PrimaryKey`, which is the *column-enum* type.
    /// This associated type is the runtime *value* — e.g. `i64` for a `BIGINT` PK or
    /// `(i32, i32)` for a composite key.
    ///
    /// *plan §5 reference*: "dispatch-tag = Entity::PK value".
    type ActorPrimaryKey: Eq + Hash + Clone + Send + Sync + 'static;

    /// Resolve the `ActorRef` for `pk`, spawning the actor if it is not already alive.
    ///
    /// Internally delegates to the entity's [`crate::registry::EntityActorRegistry`]
    /// static.  The registry is keyed on `Self::ActorPrimaryKey` and backed by a
    /// `RwLock<HashMap<…>>`.
    ///
    /// *plan §5 reference*: "`Entity::actor(pk)` — the single public entry-point for
    /// consumers".
    fn actor(pk: <Self as EntityActor>::ActorPrimaryKey) -> ractor::ActorRef<Self::ActorMsg>;
}
