use std::{any::Any, fmt, future::Future};

use crate::loader::BoxFuture;

/// An error produced while spawning or awaiting a [`Spawner`] job.
#[derive(Debug)]
pub enum SpawnError {
    /// The job was cancelled before it produced a result.
    Cancelled,
    /// The spawner backend failed for a reason of its own.
    Backend(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpawnError::Cancelled => write!(f, "spawned job was cancelled"),
            SpawnError::Backend(error) => write!(f, "spawner backend error: {error}"),
        }
    }
}

impl std::error::Error for SpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SpawnError::Cancelled => None,
            SpawnError::Backend(error) => Some(error.as_ref()),
        }
    }
}

type BoxAny = Box<dyn Any + Send>;
type BoxedJob = Box<dyn FnOnce() -> BoxAny + Send>;

/// A caller-provided destination for CPU work.
///
/// [`Spawner`] generalizes `spawn_blocking`: it decides *where* a synchronous
/// closure runs (inline, a blocking pool, a Rayon pool, a main-thread queue,
/// a game-engine job system, and so on). It knows nothing about fetch,
/// parsing, or any format-specific concern.
///
/// This trait is dyn-safe by erasing its output through [`Any`]; callers
/// should not implement or call [`Spawner::spawn`] directly outside of an
/// implementation. Use the free function [`spawn`] instead, which hides the
/// erasure and downcasting.
pub trait Spawner: Send + Sync + 'static {
    /// Runs `job` somewhere chosen by this spawner and returns a future that
    /// resolves once it completes.
    ///
    /// Implementations must not block the calling thread; if the destination
    /// (a queue, a pool, a channel) is not immediately available, they should
    /// return a future that later resolves once the job actually runs.
    fn spawn(&self, job: BoxedJob) -> BoxFuture<BoxAny, SpawnError>;
}

/// Runs `job` through `spawner` and returns its typed result.
///
/// This is the only entry point loader authors and applications need. It
/// hides the [`Any`] erasure required to keep [`Spawner`] object-safe.
///
/// # Panics
///
/// Panics if the spawner returns a value of the wrong type. This can only
/// happen if a `Spawner` implementation is broken (it must return exactly
/// what it was given), never as a result of caller-observable state.
pub fn spawn<Sp, F, T>(spawner: &Sp, job: F) -> impl Future<Output = Result<T, SpawnError>> + Send
where
    Sp: Spawner + ?Sized,
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let future = spawner.spawn(Box::new(move || Box::new(job()) as BoxAny));
    async move {
        let value = future.await?;
        Ok(*value
            .downcast::<T>()
            .expect("hiera::spawn: spawner returned a mismatched output type"))
    }
}

/// The default [`Spawner`]: runs every job synchronously and returns an
/// already-ready future.
///
/// This preserves today's behavior exactly for callers who do not opt into a
/// custom [`Spawner`]: CPU work still runs inline, on whatever thread polls
/// the loader's future.
#[derive(Debug, Default, Clone, Copy)]
pub struct InlineSpawner;

impl Spawner for InlineSpawner {
    fn spawn(&self, job: BoxedJob) -> BoxFuture<BoxAny, SpawnError> {
        Box::pin(std::future::ready(Ok(job())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn inline_spawner_runs_job_inline() {
        let ran = Arc::new(AtomicBool::new(false));
        let ran_in_job = ran.clone();

        let future = spawn(&InlineSpawner, move || {
            ran_in_job.store(true, Ordering::SeqCst);
            42
        });

        // The job already ran during `Spawner::spawn`, before this poll.
        assert!(ran.load(Ordering::SeqCst));

        let result = futures::executor::block_on(future).unwrap();
        assert_eq!(result, 42);
    }

    /// A test spawner that defers running its job until [`DeferredSpawner::drive`]
    /// is called, proving that `spawn` composes with a spawner that does not
    /// run work immediately.
    #[derive(Default, Clone)]
    struct DeferredSpawner {
        pending: Arc<Mutex<Vec<(BoxedJob, futures::channel::oneshot::Sender<BoxAny>)>>>,
    }

    impl DeferredSpawner {
        fn drive(&self) {
            let jobs = std::mem::take(&mut *self.pending.lock().unwrap());
            for (job, tx) in jobs {
                let _ = tx.send(job());
            }
        }
    }

    impl Spawner for DeferredSpawner {
        fn spawn(&self, job: BoxedJob) -> BoxFuture<BoxAny, SpawnError> {
            let (tx, rx) = futures::channel::oneshot::channel();
            self.pending.lock().unwrap().push((job, tx));
            Box::pin(async move { rx.await.map_err(|_| SpawnError::Cancelled) })
        }
    }

    #[test]
    fn deferred_spawner_only_completes_when_driven() {
        let spawner = DeferredSpawner::default();
        let mut future = Box::pin(spawn(&spawner, || "done".to_string()));

        let waker = futures::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);

        assert!(matches!(
            future.as_mut().poll(&mut cx),
            std::task::Poll::Pending
        ));

        spawner.drive();

        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(Ok(value)) => assert_eq!(value, "done"),
            other => panic!("expected Ready(Ok), got {other:?}"),
        }
    }

    #[test]
    fn dropping_pending_future_does_not_panic() {
        let spawner = DeferredSpawner::default();
        let future = spawn(&spawner, || 1_u32);
        drop(future);
        // The job is still queued but never run; dropping must not panic or
        // otherwise misbehave. Driving after drop is a no-op observable
        // effect only, proving cancellation-by-drop is safe.
        spawner.drive();
    }

    #[test]
    fn spawn_downcasts_output_type_correctly() {
        #[derive(Debug, PartialEq)]
        struct Custom(u64);

        let result = futures::executor::block_on(spawn(&InlineSpawner, || Custom(7))).unwrap();
        assert_eq!(result, Custom(7));
    }
}
