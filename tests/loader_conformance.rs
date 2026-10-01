//! Conformance tests for the [`hiera::Loader`] contract.
//!
//! These exercise the behaviour every loader implementation must provide, so
//! format crates can mirror this file against their own loaders.

use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use hiera::{
    DescribeFuture, ExpandFuture, Expansion, LoadFuture, LoadOutcome, Loader, Navigator, RootFuture,
};

/// A minimal in-memory hierarchy used to validate the contract.
#[derive(Clone)]
struct TreeLoader {
    children: Arc<HashMap<u32, Vec<u32>>>,
    contents: Arc<HashMap<u32, Vec<String>>>,
    describe_calls: Arc<AtomicUsize>,
}

impl TreeLoader {
    fn new() -> Self {
        let mut children = HashMap::new();
        children.insert(0, vec![1, 2]);
        children.insert(1, vec![3]);
        children.insert(2, vec![]);
        children.insert(3, vec![]);

        let mut contents = HashMap::new();
        contents.insert(0, vec!["root.bin".to_string()]);
        contents.insert(1, vec!["a.bin".to_string(), "b.bin".to_string()]);

        Self {
            children: Arc::new(children),
            contents: Arc::new(contents),
            describe_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Loader for TreeLoader {
    type Item = u32;
    type ItemId = u32;
    type ContentRef = String;
    type ItemInfo = usize;
    type Content = String;
    type Error = Infallible;

    fn root(&self) -> RootFuture<Self::Item, Self::Error> {
        Box::pin(async { Ok(0) })
    }

    fn item_id(&self, item: &Self::Item) -> Self::ItemId {
        *item
    }

    fn describe(&self, item: Self::Item) -> DescribeFuture<Self::ItemInfo, Self::Error> {
        let children = Arc::clone(&self.children);
        let calls = Arc::clone(&self.describe_calls);
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(children.get(&item).map(Vec::len).unwrap_or(0))
        })
    }

    fn expand(&self, item: Self::Item) -> ExpandFuture<Self::Item, Self::ContentRef, Self::Error> {
        let children = Arc::clone(&self.children);
        let contents = Arc::clone(&self.contents);
        Box::pin(async move {
            Ok(Expansion {
                children: children.get(&item).cloned().unwrap_or_default(),
                contents: contents.get(&item).cloned().unwrap_or_default(),
            })
        })
    }

    fn load(&self, content: Self::ContentRef) -> LoadFuture<Self::Content, Self::Error> {
        Box::pin(async move {
            if content.starts_with("pending") {
                return Ok(LoadOutcome::Retry {
                    after: Duration::from_millis(5),
                });
            }
            if content.is_empty() {
                return Ok(LoadOutcome::Empty);
            }
            Ok(LoadOutcome::Ready(content))
        })
    }
}

fn navigator() -> Navigator<TreeLoader, usize> {
    Navigator::with_sync_state(
        TreeLoader::new(),
        |_, _, info: &usize| Ok::<_, Infallible>(*info),
        |_, parent: &hiera::Cursor<TreeLoader, usize>, _, info: &usize| {
            Ok::<_, Infallible>(parent.state() + info)
        },
    )
}

#[test]
fn root_is_stable_across_calls() {
    futures::executor::block_on(async {
        let navigator = navigator();
        let first = navigator.root().await.unwrap();
        let second = navigator.root().await.unwrap();
        assert_eq!(first.item_id(), second.item_id());
        assert!(first.parent().is_none(), "root must have no path ancestry");
    });
}

#[test]
fn item_id_is_stable_for_the_same_item() {
    futures::executor::block_on(async {
        let loader = TreeLoader::new();
        let item = loader.root().await.unwrap();
        assert_eq!(loader.item_id(&item), loader.item_id(&item.clone()));
    });
}

#[test]
fn expansion_is_shallow() {
    futures::executor::block_on(async {
        let loader = TreeLoader::new();
        let expansion = loader.expand(0).await.unwrap();
        assert_eq!(expansion.children, vec![1, 2]);
        assert_eq!(expansion.contents, vec!["root.bin".to_string()]);

        // Expanding the root must not have expanded its children.
        let child = loader.expand(1).await.unwrap();
        assert_eq!(child.children, vec![3]);
    });
}

#[test]
fn leaf_expansion_is_empty_not_an_error() {
    futures::executor::block_on(async {
        let loader = TreeLoader::new();
        let expansion = loader.expand(2).await.unwrap();
        assert!(expansion.children.is_empty());
        assert!(expansion.contents.is_empty());
    });
}

#[test]
fn load_reports_each_outcome() {
    futures::executor::block_on(async {
        let loader = TreeLoader::new();

        match loader.load("a.bin".to_string()).await.unwrap() {
            LoadOutcome::Ready(value) => assert_eq!(value, "a.bin"),
            other => panic!("expected Ready, got {other:?}"),
        }

        match loader.load(String::new()).await.unwrap() {
            LoadOutcome::Empty => {}
            other => panic!("expected Empty, got {other:?}"),
        }

        match loader.load("pending.bin".to_string()).await.unwrap() {
            LoadOutcome::Retry { after } => assert!(after > Duration::ZERO),
            other => panic!("expected Retry, got {other:?}"),
        }
    });
}

#[test]
fn cursors_retain_path_ancestry() {
    futures::executor::block_on(async {
        let navigator = navigator();
        let root = navigator.root().await.unwrap();
        let children = root.expand().await.unwrap().children;
        let first = children.first().expect("root has children");

        assert_eq!(first.parent().map(|parent| parent.item_id()), Some(0));

        let grandchildren = first.expand().await.unwrap().children;
        let leaf = grandchildren.first().expect("child has children");
        assert_eq!(leaf.parent().map(|parent| parent.item_id()), Some(1));
        assert_eq!(
            leaf.parent()
                .and_then(|parent| parent.parent())
                .map(|root| root.item_id()),
            Some(0)
        );
    });
}

#[test]
fn cursors_are_cheap_to_retain_and_clone() {
    futures::executor::block_on(async {
        let navigator = navigator();
        let root = navigator.root().await.unwrap();
        let retained: Vec<_> = (0..16).map(|_| root.clone()).collect();
        for cursor in &retained {
            assert_eq!(cursor.item_id(), root.item_id());
        }
    });
}

#[test]
fn state_derivation_sees_parent_state() {
    futures::executor::block_on(async {
        let navigator = navigator();
        let root = navigator.root().await.unwrap();
        // The root has two children, so its derived state is 2.
        assert_eq!(*root.state(), 2);

        let children = root.expand().await.unwrap().children;
        let first = children.first().expect("root has children");
        // Child 1 has one child, accumulated onto the parent's state.
        assert_eq!(*first.state(), 3);
    });
}

#[test]
fn traversal_is_caller_driven() {
    futures::executor::block_on(async {
        let navigator = navigator();
        let root = navigator.root().await.unwrap();

        let mut visited = Vec::new();
        let mut queue = vec![root];
        while let Some(cursor) = queue.pop() {
            visited.push(cursor.item_id());
            queue.extend(cursor.expand().await.unwrap().children);
        }

        visited.sort_unstable();
        assert_eq!(visited, vec![0, 1, 2, 3]);
    });
}
