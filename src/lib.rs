//! Demand-driven loading and path-aware navigation for hierarchical spatial
//! data.
//!
//! Hiera defines format-neutral mechanics for sources such as 3D Tiles, I3S,
//! COPC, and glTF. A [`Loader`] owns source-specific loading and shallow
//! expansion. A format-specific [`Reader`] decorates a loader with typed
//! reads while retaining all loader functionality. A [`Navigator`] turns a loader into
//! retainable [`Cursor`] values whose effective state is derived by the
//! format or application.
//!
//! Hiera does not interpret bounds, transforms, level of detail, metadata, or
//! content formats. Those semantics belong to the format crate's loader and
//! state-derivation callbacks.
//!
//! ```
//! use hiera::{Expansion, LoadOutcome, Loader, Navigator};
//! use std::convert::Infallible;
//!
//! #[derive(Clone)]
//! struct Source;
//!
//! impl Loader for Source {
//!     type Item = u8;
//!     type ItemId = u8;
//!     type ContentRef = ();
//!     type ItemInfo = ();
//!     type Content = ();
//!     type Error = Infallible;
//!
//!     fn root(&self) -> hiera::RootFuture<Self::Item, Self::Error> {
//!         Box::pin(async { Ok(0) })
//!     }
//!
//!     fn item_id(&self, item: &Self::Item) -> Self::ItemId {
//!         *item
//!     }
//!
//!     fn describe(&self, _: Self::Item) -> hiera::DescribeFuture<Self::ItemInfo, Self::Error> {
//!         Box::pin(async { Ok(()) })
//!     }
//!
//!     fn expand(&self, _: Self::Item) -> hiera::ExpandFuture<Self::Item, Self::ContentRef, Self::Error> {
//!         Box::pin(async { Ok(Expansion { children: vec![], contents: vec![] }) })
//!     }
//!
//!     fn load(&self, _: Self::ContentRef) -> hiera::LoadFuture<Self::Content, Self::Error> {
//!         Box::pin(async { Ok(LoadOutcome::Empty) })
//!     }
//! }
//!
//! # async fn example() {
//! let navigator = Navigator::new(
//!     Source,
//!     |_, _, _| Box::pin(async { Ok::<_, Infallible>(()) }),
//!     |_, _, _, _| Box::pin(async { Ok::<_, Infallible>(()) }),
//! );
//! let root = navigator.root().await.unwrap();
//! assert_eq!(root.item_id(), 0);
//! # }
//! ```

#![warn(missing_docs)]

mod loader;
mod navigator;
mod reader;

pub use loader::{
    BoxFuture, ByteRange, Bytes, DescribeFuture, ExpandFuture, Expansion, Fetch, FetchError,
    FetchFuture, FetchRequest, FetchResponse, LoadFuture, LoadOutcome, Loader, RootFuture,
};
pub use navigator::{ChildState, Cursor, CursorExpansion, Navigator, RootState, StateFuture};
pub use reader::Reader;
