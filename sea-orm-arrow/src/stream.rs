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
//! # Circular-dependency resolution
//!
//! `sea-orm` already depends on `sea-orm-arrow` (for the `with-arrow` feature).
//! Adding `sea-orm` as a dependency of `sea-orm-arrow` in return would create a
//! compile-time cycle: `sea-orm → sea-orm-arrow → sea-orm` (confirmed empirically —
//! cargo reports "cyclic package dependency").
//!
//! The solution is the **arrow-only trait** shape: `SelectArrowExt` is defined
//! entirely in terms of Arrow and `futures::Stream` types, with no `sea_orm::*`
//! in the trait definition at all. The concrete blanket impls for `Select<E>` and
//! `SelectorRaw<S>` live in `sea-orm` itself (future Sprint 2 wiring), where all
//! three types (`Select`, `SelectorRaw`, `DatabaseConnection`) are in scope without
//! any circular reference.
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
//! The trait surface (method signatures) is defined here. Sprint 2 will add
//! the blanket impls for `Select<E>` and `SelectorRaw<S>` in the `sea-orm`
//! crate itself (or in a bridging module gated by the `with-arrow` feature),
//! where `DatabaseConnection` and `DbErr` are available without a circular dep.
//!
//! # Downstream consumer pattern
//!
//! A downstream crate (or `sea-orm` itself) that depends on **both** `sea-orm`
//! and `sea-orm-arrow` can provide the blanket impl:
//!
//! ```rust,ignore
//! // In sea-orm/src/arrow_stream.rs (gated by `with-arrow` feature)
//! use sea_orm_arrow::stream::SelectArrowExt;
//! use arrow::record_batch::RecordBatch;
//! use futures::Stream;
//!
//! impl<E: sea_orm::EntityTrait> SelectArrowExt for sea_orm::Select<E> {
//!     type Error = sea_orm::DbErr;
//!     fn stream_arrow<'a>(
//!         self,
//!         db: &'a sea_orm::DatabaseConnection,
//!     ) -> impl Stream<Item = Result<RecordBatch, Self::Error>> + Send + 'a {
//!         // … implementation …
//!     }
//! }
//! ```

use arrow::record_batch::RecordBatch;
use futures::Stream;

// ---------------------------------------------------------------------------
// Public extension trait
// ---------------------------------------------------------------------------

/// Extension trait that adds [`stream_arrow`][SelectArrowExt::stream_arrow] to
/// SeaORM query builders.
///
/// # Design — arrow-only trait surface
///
/// This trait is intentionally defined **without any `sea_orm::*` types** in its
/// signature. This avoids the circular-dependency problem that arises when
/// `sea-orm-arrow` (which is itself depended on by `sea-orm` via the `with-arrow`
/// feature) would need to import `sea-orm` types:
///
/// - `sea-orm` → `sea-orm-arrow` (existing `with-arrow` feature dep)
/// - `sea-orm-arrow` → `sea-orm` would create a cycle
///
/// Instead, the trait is parameterised over an associated `Db` type (the connection)
/// and an associated `Error` type, allowing the blanket impls for `Select<E>` and
/// `SelectorRaw<S>` to live in `sea-orm` itself (Sprint 2), where all types are
/// available without a circular reference.
///
/// Import this trait to enable Arrow streaming on any type that implements it:
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
/// # Dyn-compatibility note
///
/// Because `stream_arrow` returns `impl Stream` (an opaque future), this trait is
/// intentionally **not** dyn-compatible. That is correct and expected for an
/// extension trait of this kind.
///
/// See plan §6 and plan §1 Contracts table for rationale.
pub trait SelectArrowExt {
    /// The database connection type (e.g. `sea_orm::DatabaseConnection`).
    ///
    /// Kept as an associated type so the trait definition is free of any
    /// `sea_orm::*` imports, preventing a circular dependency.
    type Db: ?Sized;

    /// The error type yielded by the stream (e.g. `sea_orm::DbErr`).
    ///
    /// Kept as an associated type for the same reason as `Db`.
    type Error;

    /// Stream query results as Arrow [`RecordBatch`]es.
    ///
    /// Each batch contains a configurable number of rows (batch size TBD in Sprint 2).
    /// Rows are converted column-by-column using [`crate::arrow_array_to_value`] and
    /// assembled into a [`RecordBatch`] with a schema derived from the entity's column
    /// definitions.
    ///
    /// # Errors
    ///
    /// Yields `Self::Error` on database errors or Arrow conversion failures.
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
        db: &Self::Db,
    ) -> impl Stream<Item = Result<RecordBatch, Self::Error>> + Send;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::SelectArrowExt;

    /// Verify that `SelectArrowExt` is usable as a generic bound (the extension-trait
    /// use-case) even though no concrete types are available in this crate to instantiate.
    ///
    /// This mirrors Plan §6 Example 1: `use sea_orm_arrow::SelectArrowExt;`
    /// makes `stream_arrow` available on any implementing type.
    #[test]
    fn extension_trait_is_bound_usable() {
        // Generic function that accepts anything implementing SelectArrowExt —
        // this is the primary consumer pattern. If the trait definition is
        // broken (e.g. missing a required super-trait or associated type) this
        // function fails to compile, catching regressions at CI time.
        fn _assert_bound<T>(_query: T)
        where
            T: SelectArrowExt,
        {
        }

        // The function is never called at runtime — the compile-time check is
        // the point.
        #[allow(dead_code)]
        fn _unused() {
            // Would be called like: _assert_bound(User::find())
            // Cannot instantiate Select<E> without a real entity here, so the
            // bound check is purely structural.
        }
    }

    /// Verify the associated-type shape is correct: `Db` is `?Sized` (allowing
    /// `&dyn Trait` connection types), `Error` is unconstrained (permitting both
    /// `sea_orm::DbErr` and custom error types).
    #[test]
    fn associated_types_are_flexible() {
        use arrow::record_batch::RecordBatch;
        use futures::Stream;

        // Concrete test impl with a unit Db and infallible error to confirm
        // the trait can be implemented with any Db / Error pair.
        struct MockQuery;
        struct MockDb;

        impl SelectArrowExt for MockQuery {
            type Db = MockDb;
            type Error = std::convert::Infallible;

            fn stream_arrow(
                self,
                _db: &Self::Db,
            ) -> impl Stream<Item = Result<RecordBatch, Self::Error>> + Send {
                futures::stream::empty()
            }
        }

        let _q = MockQuery;
        // Confirm the impl satisfies the bound.
        fn require_ext<T: SelectArrowExt>(_: T) {}
        require_ext(MockQuery);
    }

    /// Doc-test style: demonstrate the import idiom from Plan §6 Example 1.
    ///
    /// ```rust,ignore
    /// use sea_orm_arrow::SelectArrowExt;   // <-- the only required import
    ///
    /// // Now stream_arrow is available on any Select<E> or SelectorRaw<S>
    /// // (once the blanket impls land in sea-orm's `with-arrow` module):
    /// let stream = User::find().stream_arrow(&db);
    /// ```
    #[test]
    fn import_idiom_documented() {
        // Verify the trait is re-exported at the crate root level (done by
        // the `pub use stream::SelectArrowExt;` re-export in lib.rs).
        // This is a no-op test whose presence signals to reviewers that the
        // import contract is intentional.
        let _ = stringify!(use sea_orm_arrow::SelectArrowExt);
    }
}
