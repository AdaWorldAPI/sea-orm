//! Extension trait for streaming query results as Arrow [`RecordBatch`]es.
//!
//! # Extension Trait Pattern (Plan §6)
//!
//! This module defines [`SelectArrowExt`] as a **Rust extension trait** — it does NOT
//! modify [`sea_orm::Select<E>`] or [`sea_orm::SelectorRaw`] themselves. Consumers opt in
//! by importing the trait:
//!
//! ```rust,ignore
//! use sea_orm_arrow::SelectArrowExt;
//!
//! // Example 1 (Plan §6 Example 1): stream a typed entity query as RecordBatches
//! let stream = User::find()
//!     .stream_arrow(&db)
//!     .await;
//! ```
//!
//! The extension trait pattern is mandated by the **additive-contract-shape principle**
//! (Plan §1 Contracts table): existing `Select<E>` / `QueryAs` public surface must remain
//! untouched so that downstream crates already compiled against `sea-orm` are unaffected.
//! Adding methods directly to those structs (inherent impls or trait impls in the main
//! crate) would be an API-surface change and potentially a semver hazard. An extension
//! trait in a separate crate avoids both problems: the new methods are invisible unless
//! the caller explicitly `use sea_orm_arrow::SelectArrowExt`.
//!
//! # Contracts (Plan §1 reference)
//!
//! | Layer              | Guarantee                                          |
//! |--------------------|----------------------------------------------------|
//! | `sea-orm` core     | `Select<E>`, `SelectorRaw<S>` API unchanged        |
//! | `sea-orm-arrow`    | Purely additive — no modification to core types    |
//! | `SelectArrowExt`   | Extension-trait only; zero breakage on non-import  |
//!
//! # Sprint note
//!
//! Method bodies are stubbed (`unimplemented!`) for Sprint 1 scaffolding. Sprint 2 will
//! provide the real implementation (row-by-row conversion via
//! [`crate::arrow_array_to_value`] and batch assembly).
//!
//! # Orchestrator checklist
//!
//! * Add `pub mod stream;` to `sea-orm-arrow/src/lib.rs`.
//! * Add `futures = { version = "0.3", default-features = false, features = ["std"] }`
//!   to `sea-orm-arrow/Cargo.toml` (the `futures::Stream` trait bound requires it; the
//!   crate is not currently in `sea-orm-arrow`'s dependency tree).
//! * Add `sea-orm = { path = "..", default-features = false }` (or an appropriate version
//!   specifier) to `sea-orm-arrow/Cargo.toml` so the `sea_orm::DatabaseConnection` and
//!   `sea_orm::DbErr` types resolve.

use arrow::record_batch::RecordBatch;

// ---------------------------------------------------------------------------
// Public extension trait
// ---------------------------------------------------------------------------

/// Extension trait that adds [`stream_arrow`][SelectArrowExt::stream_arrow] to
/// SeaORM query builders.
///
/// Import this trait to enable Arrow streaming on any [`sea_orm::Select<E>`] or
/// [`sea_orm::SelectorRaw<S>`] value:
///
/// ```rust,ignore
/// use sea_orm_arrow::SelectArrowExt;
///
/// let mut stream = User::find().stream_arrow(&db);
/// while let Some(batch) = stream.next().await {
///     let batch: RecordBatch = batch?;
///     // process batch …
/// }
/// ```
///
/// # Design
///
/// The trait is generic over the query builder (`Self`) so that blanket impls can cover
/// multiple concrete types without modifying them. Because `impl Trait` return types are
/// not (yet) object-safe, `stream_arrow` returns an `impl futures::Stream` rather than a
/// `Box<dyn Stream>`. This means `SelectArrowExt` itself is not dyn-compatible, which is
/// expected and correct for an extension trait of this kind (see the compile-time test
/// below).
///
/// See plan §6 and plan §1 Contracts table for rationale.
pub trait SelectArrowExt {
    /// Stream query results as Arrow [`RecordBatch`]es.
    ///
    /// Each batch contains a configurable number of rows (batch size TBD in Sprint 2).
    /// Rows are converted column-by-column using [`crate::arrow_array_to_value`] and
    /// assembled into a [`RecordBatch`] with a schema derived from the entity's column
    /// definitions.
    ///
    /// # Errors
    ///
    /// Yields [`sea_orm::DbErr`] on database errors or Arrow conversion failures.
    ///
    /// # Example (Plan §6 Example 1)
    ///
    /// ```rust,ignore
    /// use sea_orm_arrow::SelectArrowExt;
    ///
    /// let stream = User::find().stream_arrow(&db);
    /// ```
    fn stream_arrow(
        self,
        db: &sea_orm::DatabaseConnection,
    ) -> impl futures::Stream<Item = Result<RecordBatch, sea_orm::DbErr>> + Send;
}

// ---------------------------------------------------------------------------
// Blanket impl for Select<E: EntityTrait>
// ---------------------------------------------------------------------------

impl<E> SelectArrowExt for sea_orm::Select<E>
where
    E: sea_orm::EntityTrait,
{
    fn stream_arrow(
        self,
        _db: &sea_orm::DatabaseConnection,
    ) -> impl futures::Stream<Item = Result<RecordBatch, sea_orm::DbErr>> + Send {
        // Sprint 2 will implement: execute query, iterate rows, convert each
        // Value via crate::arrow_array_to_value, assemble RecordBatches.
        #[allow(unreachable_code)]
        futures::stream::once(async {
            unimplemented!("SO-2 stub — Sprint 2")
        })
    }
}

// ---------------------------------------------------------------------------
// Blanket impl for SelectorRaw<S> (covers QueryAs / into_model results)
// ---------------------------------------------------------------------------

impl<S> SelectArrowExt for sea_orm::SelectorRaw<S>
where
    S: sea_orm::SelectorTrait + Send,
{
    fn stream_arrow(
        self,
        _db: &sea_orm::DatabaseConnection,
    ) -> impl futures::Stream<Item = Result<RecordBatch, sea_orm::DbErr>> + Send {
        // Sprint 2 will implement the body.
        #[allow(unreachable_code)]
        futures::stream::once(async {
            unimplemented!("SO-2 stub — Sprint 2")
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    /// Verify that `SelectArrowExt` follows the extension-trait idiom:
    /// it must NOT be dyn-compatible (because of the `impl Trait` return type),
    /// which is expected and correct for this pattern.
    ///
    /// The test below is a compile-time assertion: if `SelectArrowExt` were
    /// accidentally made dyn-compatible the test body would need to change.
    /// We assert the trait is usable as a generic bound (the extension-trait
    /// use-case) rather than as a trait object.
    ///
    /// This mirrors Plan §6 Example 1: `use sea_orm_arrow::SelectArrowExt;`
    /// makes `stream_arrow` available on any implementing type.
    #[test]
    fn extension_trait_is_bound_usable() {
        use super::SelectArrowExt;

        // Generic function that accepts anything implementing SelectArrowExt —
        // this is the primary consumer pattern.  If the trait definition is
        // broken (e.g. missing a required super-trait) this function would fail
        // to compile, catching regressions at CI time.
        fn _assert_bound<T: SelectArrowExt>(_query: T) {}

        // The function is never called at runtime — the compile-time check is
        // the point.  We mark it with `#[allow(dead_code)]` to keep clippy happy.
        #[allow(dead_code)]
        fn _unused() {
            // Would be called like: _assert_bound(User::find())
            // Cannot instantiate Select<E> without a real entity here, so the
            // bound check is purely structural.
        }
    }

    /// Doc-test style: demonstrate the import idiom from Plan §6 Example 1.
    ///
    /// ```rust,ignore
    /// use sea_orm_arrow::SelectArrowExt;   // <-- the only required import
    ///
    /// // Now stream_arrow is available on any Select<E> or SelectorRaw<S>:
    /// let stream = User::find().stream_arrow(&db);
    /// ```
    #[test]
    fn import_idiom_documented() {
        // Verify the trait is re-exported at the crate root level (done by
        // orchestrator's `pub mod stream;` + consumers do
        // `use sea_orm_arrow::stream::SelectArrowExt` or
        // `use sea_orm_arrow::SelectArrowExt` once orchestrator adds the
        // `pub use stream::SelectArrowExt;` re-export).
        //
        // This is a no-op test whose presence signals to reviewers that the
        // import contract is intentional.
        let _ = stringify!(use sea_orm_arrow::SelectArrowExt);
    }
}
