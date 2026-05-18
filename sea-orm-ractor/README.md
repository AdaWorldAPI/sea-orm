# sea-orm-ractor

Actor dispatch-tag binding for [SeaORM](https://www.sea-ql.org/SeaORM) entities using the
[ractor](https://docs.rs/ractor) framework — integration-plan §5 (Glue #3).

## What this crate does

`sea-orm-ractor` binds each SeaORM entity's primary-key *value* to a live `ractor` actor
process via a global registry.  Once wired up, consumers can address any entity row as
an actor without knowing whether that actor is already running:

```rust
// Resolve (or spawn) the actor for ticket #4711, then send a message.
Ticket::Entity::actor(4711)
    .send_message(TicketMsg::Assign { user_id: 99 })
    .unwrap();
```

The call to `actor(pk)` is handled by the entity's `EntityActorRegistry` static.  On
first access for a given primary key the registry spawns the actor via the closure
supplied at construction time.  Subsequent calls for the same key return the cached
`ActorRef` after a cheap read-lock check.

## Consumer-side idiom (plan §5)

```rust
use once_cell::sync::Lazy;
use sea_orm_ractor::{EntityActor, EntityActorRegistry};

// 1. Declare your message type.
pub enum TicketMsg {
    Assign { user_id: i64 },
    Close,
}
impl ractor::Message for TicketMsg {}

// 2. Implement EntityActor for your entity.
//    (Sprint 2: replace with #[derive(SeaOrmActor)] — see note below.)
impl EntityActor for ticket::Entity {
    type ActorMsg        = TicketMsg;
    // Named ActorPrimaryKey (not PrimaryKey) to avoid ambiguity with
    // EntityTrait::PrimaryKey (column enum). This is the runtime value type.
    type ActorPrimaryKey = i64;

    fn actor(pk: i64) -> ractor::ActorRef<TicketMsg> {
        TICKET_REGISTRY.get_or_spawn(pk)
    }
}

// 3. Global registry — one per entity, initialised lazily.
static TICKET_REGISTRY: Lazy<EntityActorRegistry<ticket::Entity>> =
    Lazy::new(|| EntityActorRegistry::new(|pk| spawn_ticket_actor(pk)));

// 4. Anywhere in application code:
fn handle_assignment(ticket_id: i64, user_id: i64) {
    ticket::Entity::actor(ticket_id)
        .send_message(TicketMsg::Assign { user_id })
        .expect("actor mailbox full or actor stopped");
}
```

## Key types

| Type | Role |
|---|---|
| [`EntityActor`] | Supertrait of `sea_orm::EntityTrait`; adds `ActorMsg`, `ActorPrimaryKey`, `actor(pk)`. |
| [`EntityActorRegistry`] | `RwLock<HashMap<ActorPrimaryKey, ActorRef<Msg>>>` with `get_or_spawn` + `deregister`. |

## Derive macro — coming in Sprint 2

The `#[derive(SeaOrmActor)]` proc-macro will live in `sea-orm-macros` and emit the
`EntityActor` impl plus the `Lazy<EntityActorRegistry<…>>` static automatically.  Until
then, write the boilerplate by hand as shown above.

## Crate status

Sprint 1 stub — `EntityActorRegistry::get_or_spawn` slow-path is
`unimplemented!("SO-1 stub — Sprint 1")`.  The async `ractor::Actor::spawn` integration
(tokio handle injection + supervisor wiring) is scheduled for Sprint 2.

## License

`MIT OR Apache-2.0`
