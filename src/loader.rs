use std::{future::Future, pin::Pin};

/// A boxed, sendable future returned by a loader operation.
pub type BoxFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'static>>;

/// An error returned by a caller-provided fetch callback.
pub type FetchError = Box<dyn std::error::Error + Send + Sync>;

/// A byte range requested from a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Inclusive start offset.
    pub start: u64,
    /// Exclusive end offset, if bounded.
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
    pub bytes: Vec<u8>,
    /// The response media type, when known.
    pub content_type: Option<String>,
}

/// Future returned by a caller-provided fetch callback.
pub type FetchFuture = Pin<Box<dyn Future<Output = Result<FetchResponse, FetchError>> + Send>>;

/// A caller-provided resource fetch callback.
pub type Fetch = std::sync::Arc<dyn Fn(FetchRequest) -> FetchFuture + Send + Sync>;

/// Future returned by [`Loader::root`].
pub type RootFuture<I, E> = BoxFuture<I, E>;

/// Future returned by [`Loader::expand`].
pub type ExpandFuture<I, C, E> = BoxFuture<Expansion<I, C>, E>;

/// Future returned by [`Loader::load`].
pub type LoadFuture<T, E> = BoxFuture<LoadOutcome<T>, E>;

/// The result of expanding one item.
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
    /// Loading depends on work that has not completed; request it again later.
    Retry,
}

/// Demand-driven loading of a hierarchical data source.
///
/// kiba defines the protocol only. Implementations own their item,
/// content-reference, metadata, URI, and format-specific state types.
pub trait Loader: Send + Sync {
    /// The implementation's item/ hierarchy handle.
    type Item: Clone + Send + Sync + 'static;
    /// A reference to content belonging to an item.
    type ContentRef: Clone + Send + Sync + 'static;
    /// The decoded or raw loaded content.
    type Content: Send + 'static;
    /// The implementation's error type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Obtain the root item. This is the first demand-driven operation.
    fn root(&self) -> RootFuture<Self::Item, Self::Error>;

    /// Discover the immediate children and content references for an item.
    fn expand(&self, item: Self::Item) -> ExpandFuture<Self::Item, Self::ContentRef, Self::Error>;

    /// Load one previously discovered content reference.
    fn load(&self, content: Self::ContentRef) -> LoadFuture<Self::Content, Self::Error>;
}
