//! Typed reader composition over hierarchy loaders.

use crate::Loader;

/// A format-specific typed reader that is also a hierarchy [`Loader`].
///
/// A reader normally decorates a format loader, delegates Hiera's generic
/// hierarchy operations, and adds typed methods for its own documents,
/// metadata, and resource forms. This means a reader is accepted anywhere a
/// loader is expected without making `Loader` itself format-aware.
pub trait Reader: Loader {}
