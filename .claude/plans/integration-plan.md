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

This repo owns **#3** plus the **ontology-driven entity codegen** + the **`stream_arrow()` surface** for analytic results.

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

1. **Compile-time typed access** — `#[sea_orm::model]` + strongly-typed `COLUMN` constants make queries checked at build time. This is what `CLAUDE.md` describes as the SeaORM 2.0 walk-through.
2. **Arrow surfacing** — `sea-orm-arrow 2.0.0-rc.4` (Arrow 58) exposes results as `RecordBatch` directly. Zero-copy from Lance / DataFusion.
3. **Entity loader for relations** — `Entity::load().filter_by_*().with(...)` for nested fetches without N+1.
4. **Schema registry** — `db.get_schema_registry("crate::*").sync(db)` for entity-first deploys.
5. **Dispatch tags via `Entity::PK`** — glue #3 binds the PK to a ractor process registry.

---

## 4. Current state — file-by-file

### `src/`
Core ORM. **Stable surface** for the integration — no breaking changes planned.

### `sea-orm-arrow/`
Arrow integration (`Cargo.toml`: version 2.0.0-rc.4, Arrow 58). **The zero-copy bridge.** Needs a `stream_arrow()` method on `Select<E>` and `QueryAs` for analytic queries (see §6). Arrow 58 vs lance 57 mismatch resolved at the planner boundary (upcast).

### `sea-orm-macros/`
Derive macros. **Glue #3 lives here** as a new `#[derive(SeaOrmActor)]` macro that emits the ractor process registry binding (see §5).

### `sea-orm-codegen/`
CLI codegen for entities. **Plan**: add `--from-ontology` mode that reads `lance-graph-catalog` YAML and emits entities (see §7).

### `sea-orm-cli/`
CLI front-end. Exposes `sea-orm-cli generate entity --from-ontology schema.yml`.

### `sea-orm-sync/`
Schema sync. **Stable** — used by the entity-first workflow which the ontology codegen feeds into.

---

## 5. Glue #3 — `sea-orm-ractor`

**Goal**: every entity gets a ractor process registry binding tied to its primary key. Sending a message to "the actor that manages this entity" is a method call on the entity type.

**Why**: this is the **dispatch-tag mechanism**. The "outbound address being a dynamic tag passed implicitly via sea-orm" from the architecture discussion — given an entity PK, route a message to the actor that owns it. Removes addressing concerns from consumer code.

**Crate location**: new top-level crate `sea-orm-ractor/` + derive extension in `sea-orm-macros/`.

### Runtime API sketch

```rust
// sea-orm-ractor/src/lib.rs
use ractor::{Actor, ActorRef};
use sea_orm::EntityTrait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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

/// Every entity gets a registry via the `SeaOrmActor` derive.
pub trait EntityActor: EntityTrait {
    type ActorMsg: ractor::Message + 'static;
    type PrimaryKey: Eq + std::hash::Hash + Clone + Send + Sync + 'static;

    /// Get (or lazily spawn) the actor managing this PK.
    fn actor(pk: Self::PrimaryKey) -> ActorRef<Self::ActorMsg>;
}
```

### Derive macro sketch

In `sea-orm-macros`:

```rust
// sea-orm-macros — new derive

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

Live subscription + typed PK → actor dispatch. No polling, no addressing logic.

---

## 6. Arrow surfacing for analytic queries — `stream_arrow()`

**Goal**: when the federated planner routes an analytic query through lance-graph / DataFusion, the consumer gets the result as Arrow `RecordBatch` directly — no row-by-row materialisation.

**Why**: this is how DataFusion's per-row overhead becomes a non-event. The consumer was going to consume Arrow anyway (for tensors / dataframes / model inputs); making it the native return type means the slow path never runs.

**Already there**: `sea-orm-arrow/src/` has the column ↔ Arrow type mapping.

**Gap**: a `stream_arrow()` method on `Select<E>` and `QueryAs` that bypasses ActiveModel materialisation.

### API sketch

```rust
// sea-orm-arrow/src/stream.rs — new module
use arrow_array::RecordBatch;
use futures::Stream;
use sea_orm::{DatabaseConnection, DbErr, EntityTrait, Select};

impl<E: EntityTrait> SelectArrowExt for Select<E> {
    /// Stream results as Arrow RecordBatch instead of model rows.
    /// Returns a stream of batches sized per the connection's chunk hint.
    ///
    /// Uses column metadata from EntityTrait to build the Arrow schema,
    /// then materialises rows column-by-column instead of row-by-row.
    fn stream_arrow(self, db: &DatabaseConnection)
        -> impl Stream<Item = Result<RecordBatch, DbErr>>;
}
```

### Federation hookup

The planner (in surrealdb-core) can decide:
- PK lookup → goes through KV (sea-orm row return)
- Range / aggregate on indexed column → KV with batch read (sea-orm Arrow batch return)
- Cross-table aggregate → lance-graph / DataFusion (sea-orm Arrow streaming return)
- Cypher / graph traversal → lance-graph (sea-orm Arrow streaming return)
- Vector ANN → lance-index (sea-orm Arrow streaming return + score column)

From the consumer's perspective, this is one API. The planner picks.

---

## 7. Ontology-driven entity codegen

**Goal**: `lance-graph-catalog` is the single source of truth for schemas; sea-orm entities are generated from it.

**Why**: prevents schema drift across the three engines (surrealdb DEFINE, sea-orm Entity, lance-graph NodeShape/EdgeShape).

### CLI

```bash
sea-orm-cli generate entity --from-ontology schema.yml --output ./src/entities/
```

### Input (single source — shared with lance-graph & surrealdb)

```yaml
nodes:
  Ticket:
    pk: id
    actor: true                # opts into SeaOrmActor derive
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

Guards the bidirectional invariant: editing entities and editing the ontology converge.

---

## 8. Sprint sequence (this repo)

### Sprint 0 — Arrow version reconciliation (3 days)
- Confirm Arrow 58 alignment
- Document the version matrix in `sea-orm-arrow/README.md`: sea-orm-arrow 58, lance 57; upcast at planner boundary

### Sprint 1 — `sea-orm-ractor` MVP (2 weeks)
- New crate at `sea-orm-ractor/`
- `SeaOrmActor` derive in `sea-orm-macros`
- `EntityActorRegistry` runtime
- E2E test: `Ticket` entity, `Entity::actor(pk)` dispatch, mailbox lifecycle

### Sprint 2 — `stream_arrow()` (1 week)
- Add method to `Select<E>` and `QueryAs`
- Benchmark vs row materialisation on 1M-row table
- Document in `sea-orm-arrow` README

### Sprint 3 — `--from-ontology` codegen (2 weeks)
- Read lance-graph-catalog YAML
- Emit entity files with optional `SeaOrmActor` opt-in
- Round-trip property test
- Integrate with `sea-orm-cli generate entity`

### Sprint 4 — federation E2E (2 weeks)
- Consumer crate using sea-orm-arrow against a federated planner across all three engines
- Sample app: tiny CRM-like with tickets + users + activity timeline

---

## 9. Examples

### Example 1 — Compile-time typed query, zero-copy Arrow result

```rust
use sea_orm::entity::prelude::*;
use sea_orm::ExprTrait;
use sea_orm_arrow::SelectArrowExt;

let db: DatabaseConnection = /* ... */;

let mut stream = ticket::Entity::find()
    .filter(ticket::COLUMN.status.eq("Open"))
    .filter(ticket::COLUMN.priority.gt(7))
    .stream_arrow(&db);

while let Some(batch) = stream.next().await {
    let batch: RecordBatch = batch?;
    // Hand the Arrow batch directly to a tensor library or dataframe.
    process_columnar(&batch);
}
```

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
# Single source of truth
edit schema.yml

# Generate sea-orm entities
sea-orm-cli generate entity --from-ontology schema.yml --output ./src/entities/

# Generate surrealdb DEFINE statements
surrealdb-cli generate define --from-ontology schema.yml > deploy/schema.surql

# Apply to running surrealdb
surreal import --conn ws://surrealdb:8000 deploy/schema.surql

# Build the consumer crate
cargo build
```

One file edited; three engines stay in sync.

### Example 4 — Federated query routed by the planner

```rust
// Consumer code: doesn't know which engine runs which operator.
let results = ticket::Entity::find()
    .filter(ticket::COLUMN.status.eq("Open"))
    // .knows() generates a graph traversal — planner routes to lance-graph.
    .related_via_graph(user::Entity, "REPORTS_TO", max_hops = 3)
    // Aggregate routes to DataFusion.
    .aggregate(ticket::COLUMN.priority.avg())
    .stream_arrow(&db);

// Single API, three engines underneath, Arrow result, compile-time-typed entity.
```

---

## 10. Open questions

1. **`EntityActorRegistry` scope** — process-local for now (single-node ractor); distributed becomes v2.
2. **Composite primary keys** — derive macro tuples them; `Entity::actor((a, b))` is the API. Slightly less ergonomic, accepted.
3. **Actor lifecycle** — lazy spawn on first message; idle-timeout shutdown via supervisor. The supervisor lives in the consumer crate, not in sea-orm-ractor — sea-orm-ractor only provides the registry, not the supervision tree.
4. **Arrow 57 vs 58** — currently sea-orm-arrow is on 58 and surrealdb-core on 57. Planner upcasts at the boundary; revisit if surrealdb-core moves to 58.
5. **`stream_arrow` for write-side** — not yet planned. Writes via ActiveModel stay row-oriented (they're transactional). Open to revisit if bulk-load workloads appear.

---

## 11. Cross-references

- **Glue #1** (surrealdb-ractor — pairs with our SeaOrmActor): `AdaWorldAPI/surrealdb:.claude/plans/integration-plan.md` §5
- **Glue #2** (TiKV TableProvider): `AdaWorldAPI/lance-graph:.claude/plans/integration-plan.md` §5
- **Glue #4** (cognitive shader actor): `AdaWorldAPI/lance-graph:.claude/plans/integration-plan.md` §6
- **SIMD kernels**: `AdaWorldAPI/ndarray:.claude/plans/integration-plan.md`
- **Catalog format** (source of truth for entity codegen): `AdaWorldAPI/lance-graph:.claude/plans/integration-plan.md` §4 + §7 Sprint 3
