use std::{future::Future, hash::Hash, pin::Pin, time::Duration};

pub use bytes::Bytes;

/// A boxed, sendable future returned by a Hiera operation.
///
/// Hiera uses this alias instead of `async fn` trait methods so [`Loader`]
/// remains compatible with trait objects and implementations do not need an
/// `async-trait` dependency. Implementations must move all required values
/// into the returned `'static` future.
pub type BoxFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'static>>;

/// An error returned by a caller-provided fetch callback.
///
/// This preserves transport-specific errors without making Hiera depend on a
/// particular transport implementation.
pub type FetchError = Box<dyn std::error::Error + Send + Sync>;

/// A byte range requested from a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Inclusive zero-based byte offset.
    pub start: u64,
    /// Exclusive byte offset, if the request has an upper bound.
    pub end: Option<u64>,
}

/// Input supplied to a caller-provided fetch callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    /// The resolved resource location.
    pub uri: String,
    /// An optional byte range.
    pub range: Option<ByteRange>,
}

/// Result returned by a caller-provided fetch callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    /// The resource bytes.
    pub bytes: Bytes,
    /// The response media type, when known.
    pub content_type: Option<String>,
}

/// Future returned by a caller-provided fetch callback.
pub type FetchFuture = Pin<Box<dyn Future<Output = Result<FetchResponse, FetchError>> + Send>>;

/// A caller-provided asynchronous resource transport.
///
/// Loaders receive this callback from an application so they can resolve
/// format-specific resources without choosing an HTTP, filesystem, archive,
/// cache, or authentication implementation.
pub type Fetch = std::sync::Arc<dyn Fn(FetchRequest) -> FetchFuture + Send + Sync>;

/// Future returned by [`Loader::root`].
pub type RootFuture<I, E> = BoxFuture<I, E>;

/// Future returned by [`Loader::expand`].
pub type ExpandFuture<I, C, E> = BoxFuture<Expansion<I, C>, E>;

/// Future returned by [`Loader::describe`].
pub type DescribeFuture<D, E> = BoxFuture<D, E>;

/// Future returned by [`Loader::load`].
pub type LoadFuture<T, E> = BoxFuture<LoadOutcome<T>, E>;

/// One shallow expansion of a hierarchy item.
///
/// Expansion is deliberately shallow: it discovers the item's immediate
/// children and content references, but never recursively traverses them.
/// The item and content types belong to the loader implementation, so format
/// specific metadata remains available to its callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expansion<I, C> {
    /// The item's immediate children.
    pub children: Vec<I>,
    /// Content references discovered for the item.
    pub contents: Vec<C>,
}

/// The result of loading one content reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome<T> {
    /// Content was loaded successfully.
    Ready(T),
    /// The reference identifies content with no payload.
    Empty,
    /// Loading depends on source-local work that has not completed.
    ///
    /// The delay is source data, not a scheduling instruction. Callers remain
    /// responsible for priority, admission, cancellation, and retry policy.
    Retry {
        /// Earliest time at which loading may be retried.
        after: Duration,
    },
}

/// Demand-driven access to a retained hierarchical spatial source.
///
/// A loader owns the source-native item handle, content reference, metadata,
/// URI, cache, and format-specific state types. It exposes only shallow
/// operations: callers choose traversal strategy, scheduling, cancellation,
/// de-duplication, and content admission.
///
/// Implementations should return handles that remain valid for the loader's
/// lifetime. [`Navigator`](crate::Navigator) clones those handles into
/// retainable, path-aware cursors.
pub trait Loader: Send + Sync {
    /// Retained handle for one source-native hierarchy item.
    type Item: Clone + Send + Sync + 'static;
    /// Stable identity for a hierarchy item.
    ///
    /// Item handles may include caches or other retained implementation state
    /// that is unsuitable for equality. Navigation uses this identity to
    /// distinguish a cursor from its siblings and callers can use it to
    /// de-duplicate graph traversal frontiers.
    type ItemId: Clone + Eq + Hash + Send + Sync + 'static;
    /// Reference to content associated with an item.
    type ContentRef: Clone + Send + Sync + 'static;
    /// Immutable source-native facts about an item.
    ///
    /// Hiera deliberately does not prescribe the data: a spatial runtime may
    /// project it into bounds and transforms, while another consumer may use
    /// it for a wholly different purpose.
    type ItemInfo: Send + Sync + 'static;
    /// Decoded or raw content returned by [`Self::load`].
    type Content: Send + 'static;
    /// Error produced by source access or format interpretation.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Obtains the hierarchy root.
    ///
    /// This is the first demand-driven operation. Constructing a loader
    /// should normally perform no I/O.
    fn root(&self) -> RootFuture<Self::Item, Self::Error>;

    /// Returns the stable identity for an item.
    fn item_id(&self, item: &Self::Item) -> Self::ItemId;

    /// Resolve immutable source-native facts for one retained item.
    ///
    /// This is asynchronous because a lightweight item handle may address
    /// metadata that must be fetched or populated from a source cache.
    fn describe(&self, item: Self::Item) -> DescribeFuture<Self::ItemInfo, Self::Error>;

    /// Discovers immediate children and directly associated content.
    ///
    /// This operation must not recursively expand descendants.
    fn expand(&self, item: Self::Item) -> ExpandFuture<Self::Item, Self::ContentRef, Self::Error>;

    /// Load one previously discovered content reference.
    fn load(&self, content: Self::ContentRef) -> LoadFuture<Self::Content, Self::Error>;
}
