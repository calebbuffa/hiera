//! Retainable, format-neutral navigation over a [`crate::Loader`] hierarchy.

use std::sync::Arc;

use crate::{BoxFuture, Loader};

/// Future returned by a navigation-state derivation callback.
///
/// The state belongs to the format or application. Hiera retains it as an
/// immutable cursor snapshot but never interprets it.
pub type StateFuture<S, E> = BoxFuture<S, E>;

/// Callback that derives effective state for the root cursor.
pub type RootState<L, S> = dyn Fn(&L, &<L as Loader>::Item, &<L as Loader>::ItemInfo) -> StateFuture<S, <L as Loader>::Error>
    + Send
    + Sync;

/// Callback that derives effective state for a child cursor.
///
/// The parent represents the path used to reach the child; it is not assumed
/// to be the item's only possible parent in the source.
pub type ChildState<L, S> = dyn Fn(
        &L,
        &Cursor<L, S>,
        &<L as Loader>::Item,
        &<L as Loader>::ItemInfo,
    ) -> StateFuture<S, <L as Loader>::Error>
    + Send
    + Sync;

struct NavigatorInner<L, S>
where
    L: Loader,
{
    loader: Arc<L>,
    derive_root: Arc<RootState<L, S>>,
    derive_child: Arc<ChildState<L, S>>,
}

/// Creates retainable cursors over a demand-loaded hierarchy.
///
/// `S` is format-defined effective state. The navigator derives a fresh,
/// immutable snapshot for the root and every child. It does not interpret
/// transforms, bounds, metadata, extensions, or other format-specific
/// behavior.
pub struct Navigator<L, S>
where
    L: Loader,
{
    inner: Arc<NavigatorInner<L, S>>,
}

impl<L, S> Clone for Navigator<L, S>
where
    L: Loader,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Frame<I, S> {
    item: I,
    state: Arc<S>,
    parent: Option<Arc<Frame<I, S>>>,
}

/// A retainable, path-aware position in a demand-loaded hierarchy.
///
/// A cursor owns a shared frame containing its item handle, effective state,
/// and ancestry. Cloning it is cheap and preserves a snapshot of that
/// context, making it suitable for caller-owned BFS, DFS, indexing, and
/// conversion work queues.
pub struct Cursor<L, S>
where
    L: Loader,
{
    navigator: Navigator<L, S>,
    frame: Arc<Frame<L::Item, S>>,
}

impl<L, S> Clone for Cursor<L, S>
where
    L: Loader,
{
    fn clone(&self) -> Self {
        Self {
            navigator: self.navigator.clone(),
            frame: Arc::clone(&self.frame),
        }
    }
}

/// Result of shallowly expanding a cursor.
pub struct CursorExpansion<L, S>
where
    L: Loader,
{
    /// Retainable cursors for the expanded item's immediate children.
    pub children: Vec<Cursor<L, S>>,
    /// Content references discovered for the expanded item.
    pub contents: Vec<L::ContentRef>,
}

impl<L, S> Navigator<L, S>
where
    L: Loader + 'static,
    S: Send + Sync + 'static,
{
    /// Creates a navigator over `loader`.
    ///
    /// The callbacks derive format-specific effective state as cursors enter
    /// the root and its descendants. They are asynchronous so state
    /// derivation may resolve source-native data when needed.
    pub fn new<Root, Child>(loader: L, derive_root: Root, derive_child: Child) -> Self
    where
        Root: Fn(&L, &L::Item, &L::ItemInfo) -> StateFuture<S, L::Error> + Send + Sync + 'static,
        Child: Fn(&L, &Cursor<L, S>, &L::Item, &L::ItemInfo) -> StateFuture<S, L::Error>
            + Send
            + Sync
            + 'static,
    {
        Self::from_shared(Arc::new(loader), derive_root, derive_child)
    }

    /// Creates a navigator from a shared loader.
    pub fn from_shared<Root, Child>(loader: Arc<L>, derive_root: Root, derive_child: Child) -> Self
    where
        Root: Fn(&L, &L::Item, &L::ItemInfo) -> StateFuture<S, L::Error> + Send + Sync + 'static,
        Child: Fn(&L, &Cursor<L, S>, &L::Item, &L::ItemInfo) -> StateFuture<S, L::Error>
            + Send
            + Sync
            + 'static,
    {
        Self {
            inner: Arc::new(NavigatorInner {
                loader,
                derive_root: Arc::new(derive_root),
                derive_child: Arc::new(derive_child),
            }),
        }
    }

    /// Returns the underlying demand loader.
    pub fn loader(&self) -> &L {
        &self.inner.loader
    }

    /// Resolves and returns the root cursor.
    pub async fn root(&self) -> Result<Cursor<L, S>, L::Error> {
        let item = self.inner.loader.root().await?;
        let info = self.inner.loader.describe(item.clone()).await?;
        let state = (self.inner.derive_root)(&self.inner.loader, &item, &info).await?;

        Ok(Cursor {
            navigator: self.clone(),
            frame: Arc::new(Frame {
                item,
                state: Arc::new(state),
                parent: None,
            }),
        })
    }
}

impl<L, S> Cursor<L, S>
where
    L: Loader + 'static,
    S: Send + Sync + 'static,
{
    /// Returns the source-native item at this cursor position.
    pub fn item(&self) -> &L::Item {
        &self.frame.item
    }

    /// Returns the effective format-specific state at this cursor position.
    pub fn state(&self) -> &S {
        &self.frame.state
    }

    /// Returns this cursor's stable hierarchy identity.
    pub fn item_id(&self) -> L::ItemId {
        self.navigator.inner.loader.item_id(self.item())
    }

    /// Returns the cursor by which this position was reached, if any.
    ///
    /// The returned cursor is path ancestry, not necessarily an intrinsic
    /// unique parent in the source data.
    pub fn parent(&self) -> Option<Self> {
        self.frame.parent.as_ref().map(|frame| Self {
            navigator: self.navigator.clone(),
            frame: Arc::clone(frame),
        })
    }

    /// Performs one shallow expansion of this cursor.
    pub async fn expand(&self) -> Result<CursorExpansion<L, S>, L::Error> {
        let expansion = self
            .navigator
            .inner
            .loader
            .expand(self.frame.item.clone())
            .await?;
        let parent = Arc::clone(&self.frame);
        let mut children = Vec::with_capacity(expansion.children.len());

        for item in expansion.children {
            let info = self.navigator.inner.loader.describe(item.clone()).await?;
            let state = (self.navigator.inner.derive_child)(
                &self.navigator.inner.loader,
                self,
                &item,
                &info,
            )
            .await?;
            children.push(Self {
                navigator: self.navigator.clone(),
                frame: Arc::new(Frame {
                    item,
                    state: Arc::new(state),
                    parent: Some(Arc::clone(&parent)),
                }),
            });
        }

        Ok(CursorExpansion {
            children,
            contents: expansion.contents,
        })
    }

    /// Returns immediate child cursors in source-defined order.
    pub async fn children(&self) -> Result<Vec<Self>, L::Error> {
        Ok(self.expand().await?.children)
    }

    /// Returns every child of this cursor's path parent except this cursor.
    ///
    /// The root has no siblings and returns an empty vector.
    pub async fn siblings(&self) -> Result<Vec<Self>, L::Error> {
        let Some(parent) = self.parent() else {
            return Ok(Vec::new());
        };
        let item_id = self.item_id();
        Ok(parent
            .children()
            .await?
            .into_iter()
            .filter(|candidate| candidate.item_id() != item_id)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, io};

    use super::*;
    use crate::{DescribeFuture, ExpandFuture, Expansion, LoadFuture, LoadOutcome, RootFuture};

    struct TestLoader {
        children: HashMap<u8, Vec<u8>>,
    }

    impl Loader for TestLoader {
        type Item = u8;
        type ItemId = u8;
        type ContentRef = u8;
        type ItemInfo = u8;
        type Content = u8;
        type Error = io::Error;

        fn root(&self) -> RootFuture<Self::Item, Self::Error> {
            Box::pin(async { Ok(1) })
        }

        fn item_id(&self, item: &Self::Item) -> Self::ItemId {
            *item
        }

        fn describe(&self, item: Self::Item) -> DescribeFuture<Self::ItemInfo, Self::Error> {
            Box::pin(async move { Ok(item) })
        }

        fn expand(
            &self,
            item: Self::Item,
        ) -> ExpandFuture<Self::Item, Self::ContentRef, Self::Error> {
            let children = self.children.get(&item).cloned().unwrap_or_default();
            Box::pin(async move {
                Ok(Expansion {
                    contents: vec![item],
                    children,
                })
            })
        }

        fn load(&self, content: Self::ContentRef) -> LoadFuture<Self::Content, Self::Error> {
            Box::pin(async move { Ok(LoadOutcome::Ready(content)) })
        }
    }

    #[test]
    fn cursors_retain_state_ancestry_and_contents() {
        futures::executor::block_on(async {
            let navigator = Navigator::new(
                TestLoader {
                    children: HashMap::from([(1, vec![2, 3]), (2, vec![4])]),
                },
                |_loader, item, _info| {
                    let item = *item;
                    Box::pin(async move { Ok(vec![item]) })
                },
                |_loader, parent, item, _info| {
                    let mut state = parent.state().clone();
                    state.push(*item);
                    Box::pin(async move { Ok(state) })
                },
            );

            let root = navigator.root().await.unwrap();
            assert_eq!(root.state(), &[1]);
            assert!(root.parent().is_none());

            let expansion = root.expand().await.unwrap();
            assert_eq!(expansion.contents, [1]);
            let left = expansion.children[0].clone();
            assert_eq!(left.state(), &[1, 2]);
            assert_eq!(left.parent().unwrap().item(), &1);

            let siblings = left.siblings().await.unwrap();
            assert_eq!(siblings.len(), 1);
            assert_eq!(siblings[0].item(), &3);
            assert_eq!(siblings[0].state(), &[1, 3]);
        });
    }
}
