# Integration Plan: sea-orm's role in the four-repo convergence

**This repo**: `AdaWorldAPI/sea-orm` — typed entity layer + Arrow surface + dispatch-tag router.

**Status**: planning document. Companion plans at the same path in the other repos:
- `AdaWorldAPI/lance-graph:.claude/plans/integration-plan.md`
- `AdaWorldAPI/surrealdb:.claude/plans/integration-plan.md`
- `AdaWorldAPI/ndarray:.claude/plans/integration-plan.md`

---

## 1. The convergence target

Across all four repos:

> *Foundry-style ontology + BEAM-style supervision + ClickHouse-style analytic + Postgres-style ACID + cognitive primitives — all on one Arrow substrate, surfaced to consumers as a typed sea-orm API.*

Four glue crates close the gap:

| # | Glue crate | Owner repo | Bridges |
|---|---|---|---|
| 1 | `surrealdb-ractor` | surrealdb | `cf` / live queries → ractor mailboxes |
| 2 | `lance-graph-tikv-provider` | lance-graph | TiKV ranges → Arrow `TableProvider` |
| 3 | `sea-orm-ractor` | **this repo** | `Entity::PK` → ractor process registry |
| 4 | `cognitive-shader-actor` | lance-graph | cognitive shaders → `ractor::Actor` adapter |

This repo owns **#3** plus the **ontology-driven entity codegen** + the **`stream_arrow()` extension trait** for analytic results.

### Integration principle: additive contract shape

**All work in this plan is additive.** No existing trait signature changes. No existing module moves. No existing file deletes. New capabilities ship as **new traits** (`EntityActor`), **new extension traits** (`SelectArrowExt`), **new derive macros** (`SeaOrmActor`), or **new CLI flags** (`--from-ontology`). The existing surface today's consumers depend on stays exactly as-is.

**Contract crates are the integration surface.** sea-orm's role in this discipline is the **extension-trait pattern**: don't add methods to `Select<E>`, add a `SelectArrowExt` trait that's blanket-implemented for `Select<E>`. Consumers opt in via `use sea_orm_arrow::SelectArrowExt`. Non-importers see no change.

### Contracts (existing + new)

| Contract | Owner repo | Status today | This plan adds |
|---|---|---|---|
| `lance-graph-contract` | lance-graph | 0.1.x | 0.2.0 with new IR submodule (additive); no impact on sea-orm |
| `KVKey` / `Datastore` (surrealdb) | surrealdb | stable | new `CfStream` / `MvccSource` traits (additive); no impact on sea-orm |
| `EntityTrait` / `ColumnTrait` / `Select<E>` / `QueryAs` | **this repo** | 2.0 | **unchanged** — new trait `EntityActor` + new extension trait `SelectArrowExt` + new derive `SeaOrmActor` |
| `ndarray::hpc::*` | ndarray | 0.17 fork | unchanged |

**New surfaces this repo adds** (all opt-in):

```rust
// sea-orm-ractor/src/lib.rs                    — NEW crate
/// Extends EntityTrait. Implementors get a per-PK actor registry.
pub trait EntityActor: sea_orm::EntityTrait {
    type ActorMsg: ractor::Message + 'static;
    type PrimaryKey: Eq + std::hash::Hash + Clone + Send + Sync + 'static;
    fn actor(pk: Self::PrimaryKey) -> ractor::ActorRef<Self::ActorMsg>;
}

// sea-orm-arrow/src/stream.rs                  — NEW module in existing crate
/// Extension trait — blanket-impl'd for Select<E> and QueryAs.
/// Existing Select<E> / QueryAs surfaces are UNCHANGED. Consumers opt in via `use`.
pub trait SelectArrowExt {
    fn stream_arrow(self, db: &sea_orm::DatabaseConnection)
        -> impl futures::Stream<Item = Result<arrow_array::RecordBatch, sea_orm::DbErr>>;
}

// sea-orm-macros                                — NEW derive added; existing derives unchanged
#[proc_macro_derive(SeaOrmActor, attributes(actor))]
pub fn derive_sea_orm_actor(input: TokenStream) -> TokenStream { /* ... */ }
```

**Per-repo enforcement**: every Sprint item below is read as "add this; don't change what's there."

---

## 2. Architecture diagram

```
                ┌──────────────────────────────────────────┐
                │              consumer crate              │
                └──────────────────┬───────────────────────┘
                                   │ typed entities
                                   ▼
                ┌──────────────────────────────────────────┐
                │            THIS REPO (sea-orm)           │  (planner-aware ORM)
                │  - 2.0 typed COLUMN constants            │
                │  - sea-orm-arrow Arrow surface           │
                │  - sea-orm-ractor dispatch registry      │
                └────┬─────────────────┬───────────────┬───┘
                     │                 │               │
                     ▼                 ▼               ▼
              ┌───────────┐     ┌───────────┐    ┌───────────┐
              │  ractor   │◄────│ surrealdb │    │lance-graph│
              │ (actors,  │ #1  │  (cf +    │    │ (Cypher,  │
              │ mailboxes,│     │   live    │    │ ontology, │
              │ supervis.)│     │  queries) │    │cognitive) │
              └─────┬─────┘     └─────┬─────┘    └─────┬─────┘
                    │ #3              │                │ #2,#4
                    ▼                 ▼                ▼
              ┌─────────────────────────────────────────────┐
              │       TiKV substrate (Raft + Percolator)    │
              └─────────────────────────────────────────────┘
                                  │
                                  ▼
                    ┌────────────────────────────┐
                    │  ndarray fork (SIMD HPC)   │
                    └────────────────────────────┘
```

---

## 3. Role of sea-orm in the integration

This repo is **the consumer-facing API**. Consumers don't pick engines — sea-orm 2.0's typed entities + the federated planner do that. The contract sea-orm provides:

1. **Compile-time typed access** — `#[sea_orm::model]` + strongly-typed `COLUMN` constants make queries checked at build time (per `CLAUDE.md`)
2. **Arrow surfacing** — `sea-orm-arrow` 2.0.0-rc.4 exposes results as `RecordBatch` (extension trait `SelectArrowExt`)
3. **Entity loader for relations** — `Entity::load().filter_by_*().with(...)` for nested fetches without N+1
4. **Schema registry** — `db.get_schema_registry("crate::*").sync(db)` for entity-first deploys
5. **Dispatch tags via `Entity::PK`** — glue #3 binds the PK to a ractor process registry via the new `EntityActor` trait

Every one of (1)–(4) stays exactly as it is. (5) is the new opt-in surface.

---

## 4. Current state — file-by-file

### `src/`
Core ORM. **Stable surface** for the integration — no breaking changes planned.

### `sea-orm-arrow/`
Arrow integration. Today 2.0.0-rc.4 / Arrow 58. **The zero-copy bridge.** Gets a NEW module `sea-orm-arrow/src/stream.rs` defining `SelectArrowExt`; existing `sea-orm-arrow` surface is unchanged.

### `sea-orm-macros/`
Derive macros. Adds NEW `#[derive(SeaOrmActor)]`; existing derives unchanged.

### `sea-orm-codegen/`
CLI codegen. Adds NEW `--from-ontology` mode that reads `lance-graph-catalog` YAML and emits entities (§7). Existing flags unchanged.

### `sea-orm-cli/`
Exposes the new `--from-ontology` flag on the existing `generate entity` subcommand. No existing flags changed.

### `sea-orm-sync/`
Stable. Entity-first workflow which the ontology codegen feeds into.

---

## 5. Glue #3 — `sea-orm-ractor`

**Goal**: every entity gets a ractor process registry binding tied to its primary key. Sending a message to "the actor that manages this entity" is a method call on the entity type.

**Why**: the **dispatch-tag mechanism**. The "outbound address being a dynamic tag passed implicitly via sea-orm" — given an entity PK, route a message to the actor that owns it. Removes addressing concerns from consumer code.

**Additive shape**: NEW top-level crate `sea-orm-ractor/` + a NEW derive in `sea-orm-macros/`. The new `EntityActor` trait extends `EntityTrait` — it doesn't modify it. Entities that don't `#[derive(SeaOrmActor)]` are unaffected.

**Crate location**: new top-level crate `sea-orm-ractor/` + derive extension in `sea-orm-macros/`.

### Runtime API sketch

```rust
// sea-orm-ractor/src/lib.rs
use ractor::{Actor, ActorRef};
use sea_orm::EntityTrait;
use std::collections::HashMap;
use std::sync::RwLock;

/// A typed actor registry keyed on an entity's primary key.
pub struct EntityActorRegistry<E: EntityActor> {
    map: RwLock<HashMap<E::PrimaryKey, ActorRef<E::ActorMsg>>>,
    spawn: Box<dyn Fn(E::PrimaryKey) -> ActorRef<E::ActorMsg> + Send + Sync>,
}

impl<E: EntityActor> EntityActorRegistry<E> {
    pub fn get_or_spawn(&self, pk: E::PrimaryKey) -> ActorRef<E::ActorMsg> {
        if let Some(actor) = self.map.read().unwrap().get(&pk).cloned() {
            return actor;
        }
        let actor = (self.spawn)(pk.clone());
        self.map.write().unwrap().insert(pk, actor.clone());
        actor
    }
}

/// NEW trait. Extends EntityTrait; doesn't replace it.
/// Only entities with `#[derive(SeaOrmActor)]` implement this.
pub trait EntityActor: EntityTrait {
    type ActorMsg: ractor::Message + 'static;
    type PrimaryKey: Eq + std::hash::Hash + Clone + Send + Sync + 'static;
    fn actor(pk: Self::PrimaryKey) -> ActorRef<Self::ActorMsg>;
}
```

### Derive macro sketch

In `sea-orm-macros`:

```rust
// sea-orm-macros — NEW derive; existing derives unchanged

#[proc_macro_derive(SeaOrmActor, attributes(actor))]
pub fn derive_sea_orm_actor(input: TokenStream) -> TokenStream {
    // 1. Parse the entity model + the `#[actor(msg = "...")]` attribute.
    // 2. Emit:
    //    - impl EntityActor for Entity with ActorMsg = the named msg type
    //    - a static once_cell::Lazy<EntityActorRegistry<Entity>>
    //    - the actor() method that proxies into the lazy static
}
```

### Usage — the "wow" moment

```rust
use sea_orm::entity::prelude::*;
use sea_orm_ractor::SeaOrmActor;

mod ticket {
    use sea_orm::entity::prelude::*;
    use sea_orm_ractor::SeaOrmActor;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, SeaOrmActor)]
    #[sea_orm(table_name = "ticket")]
    #[actor(msg = "TicketMsg")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub status: String,
        pub priority: i32,
        #[sea_orm(belongs_to, from = "owner_id", to = "id")]
        pub owner: HasOne<super::user::Entity>,
    }

    #[derive(Debug)]
    pub enum TicketMsg {
        Assign(i64),
        Resolve,
        Escalate,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

// Consumer code — the outbound-address-as-dynamic-tag moment:
let actor = ticket::Entity::actor(4711);
actor.send_message(ticket::TicketMsg::Assign(user_id))?;
```

Entities without `#[derive(SeaOrmActor)]` continue to work unchanged. The trait is opt-in.

### Combined with surrealdb-ractor (glue #1)

The Hiro-ticket-system replacement in three lines:

```rust
use futures::StreamExt;
use surrealdb_ractor::live_stream;

let mut deltas = live_stream(
    "SELECT * FROM ticket WHERE status = 'open' AND priority > 7"
).await?;

while let Some(delta) = deltas.next().await {
    let ticket_id: i64 = delta?.primary_key("id")?;
    ticket::Entity::actor(ticket_id)
        .send_message(ticket::TicketMsg::Escalate)?;
}
```

---

## 6. Arrow surfacing — `stream_arrow()` via `SelectArrowExt`

**Goal**: when the federated planner routes an analytic query through lance-graph / DataFusion, the consumer gets the result as Arrow `RecordBatch` directly — no row-by-row materialisation.

**Why**: this is how DataFusion's per-row overhead becomes a non-event. The consumer was going to consume Arrow anyway (for tensors / dataframes / model inputs); making it the native return type means the slow path never runs.

**Additive shape**: **extension trait pattern**. `SelectArrowExt` is a NEW trait blanket-implemented for `Select<E>` and `QueryAs`. Consumers opt in by `use sea_orm_arrow::SelectArrowExt`. The existing `Select<E>` / `QueryAs` surfaces are unchanged.

### API sketch (the additive way)

```rust
// sea-orm-arrow/src/stream.rs — NEW module in existing crate
use arrow_array::RecordBatch;
use futures::Stream;
use sea_orm::{DatabaseConnection, DbErr, EntityTrait, Select, QueryAs};

/// Extension trait. Importing this in a consumer enables `select.stream_arrow(&db)`.
/// Existing `Select<E>` surface is UNCHANGED. Non-importers see no API change.
pub trait SelectArrowExt {
    fn stream_arrow(self, db: &DatabaseConnection)
        -> impl Stream<Item = Result<RecordBatch, DbErr>> + Send;
}

impl<E: EntityTrait> SelectArrowExt for Select<E> {
    fn stream_arrow(self, db: &DatabaseConnection)
        -> impl Stream<Item = Result<RecordBatch, DbErr>> + Send
    {
        // Uses column metadata from EntityTrait to build the Arrow schema,
        // then materialises rows column-by-column.
        materialise_arrow_columnar::<E>(self, db)
    }
}

// blanket impl for QueryAs too...
```

### Why this matters for additive-ness

If we added `stream_arrow()` as an inherent method on `Select<E>`:
- `Select<E>` changes
- Any downstream `impl<E> Select<E>` or trait bound naming `Select<E>` could become incompatible
- The break would be invisible until consumers tried to compile

With the extension trait pattern:
- `Select<E>` itself stays exactly the same
- The new capability is gated by `use sea_orm_arrow::SelectArrowExt`
- Non-importers see literally no change
- The blanket impl works for every `E: EntityTrait` for free

This is the canonical Rust idiom for additive method extension.

### Federation hookup

The planner (in surrealdb-core) can decide:
- PK lookup → KV (sea-orm row return, no Arrow)
- Range / aggregate on indexed column → KV with batch read (Arrow batch return via `stream_arrow`)
- Cross-table aggregate → lance-graph / DataFusion (Arrow streaming return)
- Cypher / graph traversal → lance-graph (Arrow streaming return)
- Vector ANN → lance-index (Arrow streaming return + score column)

From the consumer's perspective, this is one API. The planner picks.

---

## 7. Ontology-driven entity codegen

**Goal**: `lance-graph-catalog` is the single source of truth for schemas; sea-orm entities are generated from it.

**Why**: prevents schema drift across the three engines.

**Additive shape**: NEW `--from-ontology` flag added to the existing `sea-orm-cli generate entity` subcommand. Existing flags / behaviour unchanged.

### CLI

```bash
sea-orm-cli generate entity --from-ontology schema.yml --output ./src/entities/
```

### Input (shared with lance-graph & surrealdb)

```yaml
nodes:
  Ticket:
    pk: id
    actor: true                # opts the entity into SeaOrmActor derive
    columns:
      id: UInt64
      status: { type: String, enum: [Open, Assigned, Resolved, Escalated] }
      priority: Int32
      owner_id: { type: UInt64, fk: User }
```

### Output

```rust
// src/entities/ticket/mod.rs — generated
use sea_orm::entity::prelude::*;
use sea_orm_ractor::SeaOrmActor;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, SeaOrmActor)]
#[sea_orm(table_name = "ticket")]
#[actor(msg = "TicketMsg")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    pub status: TicketStatus,
    pub priority: i32,
    #[sea_orm(belongs_to, from = "owner_id", to = "id")]
    pub owner: HasOne<super::user::Entity>,
}

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(16))")]
pub enum TicketStatus {
    #[sea_orm(string_value = "Open")]      Open,
    #[sea_orm(string_value = "Assigned")]  Assigned,
    #[sea_orm(string_value = "Resolved")]  Resolved,
    #[sea_orm(string_value = "Escalated")] Escalated,
}

#[derive(Debug)]
pub enum TicketMsg {
    Assign(i64),
    Resolve,
    Escalate,
}

impl ActiveModelBehavior for ActiveModel {}
```

Entities WITHOUT `actor: true` in the YAML get the existing entity codegen output exactly as today — no `SeaOrmActor` derive, no `TicketMsg` enum. Backward-compatible by construction.

### Round-trip property test

```rust
#[test]
fn ontology_round_trip() {
    let ontology = Catalog::load("./schema.yml").unwrap();
    let entities = generate_entities(&ontology);
    let re_extracted = extract_ontology_from_entities(&entities);
    assert_eq!(ontology, re_extracted);
}
```

---

## 8. Sprint sequence (this repo)

All sprints are **additive** — nothing existing changes signature, nothing existing moves, nothing existing is deleted.

### Sprint 0 — Arrow version reconciliation (3 days)
- Confirm Arrow 58 alignment
- Document the version matrix in `sea-orm-arrow/README.md`: sea-orm-arrow 58, lance 57; upcast at planner boundary

### Sprint 1 — `sea-orm-ractor` MVP (2 weeks)
- NEW crate at `sea-orm-ractor/`
- NEW `SeaOrmActor` derive in `sea-orm-macros` (existing derives untouched)
- `EntityActorRegistry` runtime
- E2E test: `Ticket` entity with derive, dispatch via `Entity::actor(pk)`, mailbox lifecycle
- Entities without the derive remain unchanged and compile clean

### Sprint 2 — `SelectArrowExt` extension trait (1 week)
- NEW module `sea-orm-arrow/src/stream.rs`
- NEW trait `SelectArrowExt` with blanket impls for `Select<E>` and `QueryAs`
- `Select<E>` / `QueryAs` themselves untouched
- Benchmark vs row materialisation on 1M-row table
- Doc the import idiom in `sea-orm-arrow/README.md`

### Sprint 3 — `--from-ontology` codegen (2 weeks)
- NEW CLI flag on existing `generate entity` subcommand
- Read lance-graph-catalog YAML (uses `lance-graph-catalog::to_sea_orm_entity()` — lance-graph plan §7 Sprint 3)
- Emit entity files with optional `SeaOrmActor` opt-in
- Round-trip property test
- Existing entity codegen output unchanged for ontologies without `actor: true`

### Sprint 4 — federation E2E (2 weeks)
- Consumer crate using `SelectArrowExt::stream_arrow` against a federated planner across all three engines
- Sample app: tiny CRM-like with tickets + users + activity timeline

---

## 9. Examples

### Example 1 — Compile-time typed query, zero-copy Arrow result (via extension trait)

```rust
use sea_orm::entity::prelude::*;
use sea_orm::ExprTrait;
use sea_orm_arrow::SelectArrowExt;     // opt-in: enables .stream_arrow()
use futures::StreamExt;

let db: DatabaseConnection = /* ... */;

let mut stream = ticket::Entity::find()
    .filter(ticket::COLUMN.status.eq("Open"))
    .filter(ticket::COLUMN.priority.gt(7))
    .stream_arrow(&db);

while let Some(batch) = stream.next().await {
    let batch: arrow_array::RecordBatch = batch?;
    process_columnar(&batch);
}
```

Callers who don't `use SelectArrowExt` still get the existing `find()` / `all()` / `one()` API exactly as before.

### Example 2 — Live query + entity-actor dispatch

```rust
use futures::StreamExt;
use surrealdb_ractor::{live_stream, LiveDelta};

let mut deltas = live_stream(
    "SELECT * FROM ticket WHERE status = 'Open'"
).await?;

while let Some(d) = deltas.next().await {
    let delta = d?;
    if let LiveDelta::Create(_) | LiveDelta::Update { .. } = &delta {
        let id: i64 = delta.primary_key("id")?;
        ticket::Entity::actor(id)
            .send_message(ticket::TicketMsg::Escalate)?;
    }
}
```

### Example 3 — Entity-first, ontology-driven deploy

```bash
edit schema.yml
sea-orm-cli generate entity --from-ontology schema.yml --output ./src/entities/
surrealdb-cli generate define --from-ontology schema.yml > deploy/schema.surql
surreal import --conn ws://surrealdb:8000 deploy/schema.surql
cargo build
```

One file edited; three engines stay in sync.

### Example 4 — Federated query routed by the planner

```rust
use sea_orm_arrow::SelectArrowExt;

let results = ticket::Entity::find()
    .filter(ticket::COLUMN.status.eq("Open"))
    .related_via_graph(user::Entity, "REPORTS_TO", max_hops = 3)
    .aggregate(ticket::COLUMN.priority.avg())
    .stream_arrow(&db);
```

Single API, three engines underneath, Arrow result, compile-time-typed entity — all via the additive extension trait.

---

## 10. Open questions

1. **`EntityActorRegistry` scope** — process-local for now (single-node ractor); distributed becomes v2.
2. **Composite primary keys** — derive macro tuples them; `Entity::actor((a, b))` is the API.
3. **Actor lifecycle** — lazy spawn on first message; idle-timeout shutdown via supervisor (which lives in the consumer crate, not in sea-orm-ractor).
4. **Arrow 57 vs 58** — sea-orm-arrow on 58 and surrealdb-core on 57. Planner upcasts at the boundary.
5. **Write-side `stream_arrow`** — not planned. Writes via ActiveModel stay row-oriented (they're transactional).
6. **Extension trait visibility** — ensure `SelectArrowExt` is re-exported from a stable path (`sea_orm_arrow::SelectArrowExt`) so consumers have a single import.

---

## 11. Cross-references

- **Glue #1** (surrealdb-ractor — pairs with our SeaOrmActor): `AdaWorldAPI/surrealdb:.claude/plans/integration-plan.md` §5
- **Glue #2** (TiKV TableProvider): `AdaWorldAPI/lance-graph:.claude/plans/integration-plan.md` §5
- **Glue #4** (cognitive shader actor): `AdaWorldAPI/lance-graph:.claude/plans/integration-plan.md` §6
- **SIMD kernels**: `AdaWorldAPI/ndarray:.claude/plans/integration-plan.md`
- **Catalog `to_sea_orm_entity()` method** (source of truth for our codegen): `AdaWorldAPI/lance-graph:.claude/plans/integration-plan.md` §4 + §7 Sprint 3
- **`lance-projection` additive sibling** (rather than kv-lance demotion): `AdaWorldAPI/surrealdb:.claude/plans/integration-plan.md` §6
