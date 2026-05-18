//! # `EntityActorRegistry` — integration-plan §5
//!
//! Runtime registry that maps a SeaORM entity's primary-key value to a live
//! `ractor::ActorRef`.  One `EntityActorRegistry<E>` instance is typically held in a
//! `once_cell::sync::Lazy` static per entity.
//!
//! Spawn logic is supplied by the caller as a closure, keeping this struct free of
//! hard-coded actor implementation details.

use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, RwLock},
};

use ractor::ActorRef;

use crate::entity_actor::EntityActor;

/// A thread-safe `ActorPrimaryKey → ActorRef` map with lazy-spawn semantics.
///
/// `E` is the entity type implementing [`EntityActor`].  The registry is generic over
/// `E` so that the Rust type system prevents mixing `ActorRef`s from different entities.
///
/// # Concurrency model (plan §5)
///
/// Reads are cheap: the `RwLock` allows multiple concurrent readers.  Writes (spawn +
/// insert) are brief — they only hold the write lock while inserting the new `ActorRef`
/// into the map.  Actual actor spawning happens *before* acquiring the write lock to
/// minimise contention.
///
/// # Sprint 1 status
///
/// `get_or_spawn` body uses `unimplemented!` because the async `ractor::Actor::spawn`
/// call requires runtime plumbing (tokio handle injection) that is out of scope for the
/// SO-1 stub.  The type signatures and concurrency model are final.
pub struct EntityActorRegistry<E>
where
    E: EntityActor,
    <E as EntityActor>::ActorPrimaryKey: Eq + Hash,
{
    /// The inner map guarded by a reader-writer lock.
    ///
    /// Wrapped in `Arc` so the registry can be cheaply cloned into `Lazy` statics or
    /// shared across threads without lifetime coupling.
    map: Arc<RwLock<HashMap<<E as EntityActor>::ActorPrimaryKey, ActorRef<<E as EntityActor>::ActorMsg>>>>,

    /// Spawn closure: given a primary-key value, produce a running `ActorRef`.
    ///
    /// The closure is stored as a boxed `Fn` so it can be called from `get_or_spawn`
    /// without `Self` needing to know the concrete actor struct.  Sprint 2 will replace
    /// `Box<dyn Fn>` with an associated type / blanket impl once the macro generates it.
    ///
    /// *plan §5 reference*: "registry holds spawn closure per entity".
    spawn_fn: Arc<dyn Fn(<E as EntityActor>::ActorPrimaryKey) -> ActorRef<<E as EntityActor>::ActorMsg> + Send + Sync + 'static>,
}

impl<E> EntityActorRegistry<E>
where
    E: EntityActor,
    <E as EntityActor>::ActorPrimaryKey: Eq + Hash + Clone,
{
    /// Create a new registry with the given spawn closure.
    ///
    /// The closure is invoked inside [`get_or_spawn`] when `pk` is not yet in the map.
    ///
    /// # Example (plan §5 consumer pattern)
    ///
    /// ```rust,ignore
    /// static REGISTRY: Lazy<EntityActorRegistry<ticket::Entity>> =
    ///     Lazy::new(|| EntityActorRegistry::new(|pk| {
    ///         // Spawn TicketActor synchronously (or via a dedicated helper).
    ///         spawn_ticket_actor(pk)
    ///     }));
    /// ```
    pub fn new<F>(spawn_fn: F) -> Self
    where
        F: Fn(<E as EntityActor>::ActorPrimaryKey) -> ActorRef<<E as EntityActor>::ActorMsg> + Send + Sync + 'static,
    {
        Self {
            map: Arc::new(RwLock::new(HashMap::new())),
            spawn_fn: Arc::new(spawn_fn),
        }
    }

    /// Return the `ActorRef` for `pk`, spawning the actor if it is not yet registered.
    ///
    /// The fast path (actor already alive) acquires only a read lock.  The slow path
    /// (first access for this `pk`) upgrades to a write lock and calls `spawn_fn`.
    ///
    /// *plan §5 reference*: "`get_or_spawn` is the single entry-point that `actor(pk)`
    /// delegates to".
    ///
    /// # Panics
    ///
    /// Panics if the `RwLock` is poisoned (indicates a previous panic while holding the
    /// lock — treat as unrecoverable).
    ///
    /// # Sprint 1 note
    ///
    /// Body is stubbed with `unimplemented!` because async spawn integration (tokio
    /// handle + ractor supervisor wiring) is deferred to Sprint 2.
    pub fn get_or_spawn(&self, pk: <E as EntityActor>::ActorPrimaryKey) -> ActorRef<<E as EntityActor>::ActorMsg> {
        // Fast path: actor already registered.
        {
            let map = self.map.read().expect("EntityActorRegistry RwLock poisoned");
            if let Some(actor_ref) = map.get(&pk) {
                return actor_ref.clone();
            }
        }

        // Slow path: need to spawn.  We do NOT hold the read lock while spawning so
        // that concurrent `get_or_spawn` calls on different PKs are not serialised.
        unimplemented!("SO-1 stub — Sprint 1: async ractor::Actor::spawn wiring pending")
    }

    /// Remove a primary key from the registry (e.g. after the actor terminates).
    ///
    /// Returns the previous `ActorRef` if one was registered, or `None`.
    ///
    /// *plan §5 reference*: "registry must support de-registration on actor exit".
    pub fn deregister(&self, pk: &<E as EntityActor>::ActorPrimaryKey) -> Option<ActorRef<<E as EntityActor>::ActorMsg>> {
        let mut map = self
            .map
            .write()
            .expect("EntityActorRegistry RwLock poisoned");
        map.remove(pk)
    }

    /// Number of currently registered actors.
    pub fn len(&self) -> usize {
        self.map
            .read()
            .expect("EntityActorRegistry RwLock poisoned")
            .len()
    }

    /// Returns `true` if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.map
            .read()
            .expect("EntityActorRegistry RwLock poisoned")
            .is_empty()
    }
}
