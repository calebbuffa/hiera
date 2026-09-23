# Hiera

Hiera provides demand-driven loading and path-aware navigation for
hierarchical spatial data.

It is the shared, format-neutral foundation for loaders such as I3S, 3D
Tiles, COPC, and glTF. Hiera owns retained item handles, shallow expansion,
content loading, cursor ancestry, and stable item identity. Format crates own
their source model and define the context accumulated while navigating it.

## Core model

- [`Loader`](https://docs.rs/hiera/latest/hiera/trait.Loader.html) resolves a
  root, describes an item, shallow-expands its children/content references,
  and loads discovered content.
- [`Navigator`](https://docs.rs/hiera/latest/hiera/struct.Navigator.html)
  derives format- or application-specific state at the root and at every
  child.
- [`Cursor`](https://docs.rs/hiera/latest/hiera/struct.Cursor.html) is a
  retainable path through the source hierarchy. It supports `parent`,
  `children`, `siblings`, and shallow `expand`.

Hiera does not prescribe transforms, bounds, LOD, metadata, refinement, or
content formats. A format crate provides those semantics in its state
derivation callbacks.

## Example

```rust
use hiera::{Expansion, LoadOutcome, Loader, Navigator};
use std::convert::Infallible;

#[derive(Clone)]
struct Source;

impl Loader for Source {
    type Item = u8;
    type ItemId = u8;
    type ContentRef = ();
    type ItemInfo = ();
    type Content = ();
    type Error = Infallible;

    fn root(&self) -> hiera::RootFuture<Self::Item, Self::Error> {
        Box::pin(async { Ok(0) })
    }

    fn item_id(&self, item: &Self::Item) -> Self::ItemId {
        *item
    }

    fn describe(&self, _: Self::Item) -> hiera::DescribeFuture<Self::ItemInfo, Self::Error> {
        Box::pin(async { Ok(()) })
    }

    fn expand(&self, _: Self::Item) -> hiera::ExpandFuture<Self::Item, Self::ContentRef, Self::Error> {
        Box::pin(async { Ok(Expansion { children: vec![], contents: vec![] }) })
    }

    fn load(&self, _: Self::ContentRef) -> hiera::LoadFuture<Self::Content, Self::Error> {
        Box::pin(async { Ok(LoadOutcome::Empty) })
    }
}

# async fn example() {
let navigator = Navigator::new(
    Source,
    |_, _, _| Box::pin(async { Ok::<_, Infallible>(()) }),
    |_, _, _, _| Box::pin(async { Ok::<_, Infallible>(()) }),
);
let root = navigator.root().await.unwrap();
assert_eq!(root.item_id(), 0);
# }
```

## Status

Hiera is an early `0.1` release. Its public API is intended for hierarchical
spatial sources, but may evolve as additional formats establish common needs.
