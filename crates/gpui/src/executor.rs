use crate::{App, ForegroundContext, NotClone, PlatformDispatcher};
use async_task::Builder;
use futures::channel::mpsc;
use smol::prelude::*;
use std::{
    fmt::Debug,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    num::NonZeroUsize,
    pin::Pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering::SeqCst},
        Arc,
    },
    task::{Context, Poll},
    thread::ThreadId,
    time::{Duration, Instant},
};
use util::TryFutureExt;
use waker_fn::waker_fn;

#[cfg(any(test, feature = "test-support"))]
use rand::rngs::StdRng;

/// A pointer to the executor that is currently running,
/// for spawning background tasks.
pub struct BackgroundExecutor {
    #[doc(hidden)]
    pub dispatcher: Arc<dyn PlatformDispatcher>,
    _not_clone: NotClone,
}

/// A pointer to the executor that is currently running,
/// for spawning tasks on the main thread.
///
/// This is intentionally `!Send` via the `not_send` marker field. This is because
/// `ForegroundExecutor::spawn` does not require `Send` but checks at runtime that the future is
/// only polled from the same thread it was spawned from. These checks would fail when spawning
/// foreground tasks from from background threads.
pub struct ForegroundExecutor {
    #[doc(hidden)]
    pub dispatcher: Arc<dyn PlatformDispatcher>,
    _not_clone: NotClone,
    not_send: PhantomData<Rc<()>>,
}

/// Task is a primitive that allows work to happen in the background.
///
/// It implements [`Future`] so you can `.await` on it.
///
/// If you drop a task it will be cancelled immediately. Calling [`Task::detach`] allows
/// the task to continue running, but with no way to return a value.
#[must_use]
#[derive(Debug)]
pub struct Task<T>(TaskState<T>);

#[derive(Debug)]
enum TaskState<T> {
    /// A task that is ready to return a value
    Ready(Option<T>),

    /// A task that is currently running on the foreground.
    ForegroundSpawned(async_task::Task<T, ForegroundContext>),

    /// A task that is currently running on the background
    BackgroundSpawned(async_task::Task<T>),
}

impl<T> Task<T> {
    /// Creates a new task that will resolve with the value
    pub fn ready(val: T) -> Self {
        Task(TaskState::Ready(Some(val)))
    }

    /// Detaching a task runs it to completion in the background
    pub fn detach(self) {
        match self {
            Task(TaskState::Ready(_)) => {}
            Task(TaskState::ForegroundSpawned(task)) => task.detach(),
            Task(TaskState::BackgroundSpawned(task)) => task.detach(),
        }
    }
}

impl<E, T> Task<Result<T, E>>
where
    T: 'static,
    E: 'static + Debug,
{
    /// Run the task to completion in the background and log any
    /// errors that occur.
    #[track_caller]
    pub fn detach_and_log_err(self, cx: &App) {
        let location = core::panic::Location::caller();
        cx.foreground_executor()
            .spawn(self.log_tracked_err(*location))
            .detach();
    }
}
impl<T> Future for Task<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match unsafe { self.get_unchecked_mut() } {
            Task(TaskState::Ready(val)) => Poll::Ready(val.take().unwrap()),
            Task(TaskState::ForegroundSpawned(task)) => task.poll(cx),
            Task(TaskState::BackgroundSpawned(task)) => task.poll(cx),
        }
    }
}

/// A task label is an opaque identifier that you can use to
/// refer to a task in tests.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TaskLabel(NonZeroUsize);

impl Default for TaskLabel {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskLabel {
    /// Construct a new task label.
    pub fn new() -> Self {
        static NEXT_TASK_LABEL: AtomicUsize = AtomicUsize::new(1);
        Self(NEXT_TASK_LABEL.fetch_add(1, SeqCst).try_into().unwrap())
    }
}

type AnyFuture<R> = Pin<Box<dyn 'static + Send + Future<Output = R>>>;

/// BackgroundExecutor lets you run things on background threads.
/// In production this is a thread pool with no ordering guarantees.
/// In tests this is simulated by running tasks one by one in a deterministic
/// (but arbitrary) order controlled by the `SEED` environment variable.
impl BackgroundExecutor {
    #[doc(hidden)]
    pub fn new(dispatcher: Arc<dyn PlatformDispatcher>) -> Self {
        Self {
            dispatcher,
            _not_clone: NotClone,
        }
    }

    /// Cloning executors can cause runtime panics, see the documentation on `NotClone` for details.
    /// Use this power wisely.
    #[doc(hidden)]
    pub fn clone(&self) -> Self {
        Self {
            dispatcher: self.dispatcher.clone(),
            _not_clone: NotClone,
        }
    }

    /// Enqueues the given future to be run to completion on a background thread.
    pub fn spawn<R>(&self, future: impl Future<Output = R> + Send + 'static) -> Task<R>
    where
        R: Send + 'static,
    {
        self.spawn_internal::<R>(Box::pin(future), None)
    }

    /// Enqueues the given future to be run to completion on a background thread.
    /// The given label can be used to control the priority of the task in tests.
    pub fn spawn_labeled<R>(
        &self,
        label: TaskLabel,
        future: impl Future<Output = R> + Send + 'static,
    ) -> Task<R>
    where
        R: Send + 'static,
    {
        self.spawn_internal::<R>(Box::pin(future), Some(label))
    }

    fn spawn_internal<R: Send + 'static>(
        &self,
        future: AnyFuture<R>,
        label: Option<TaskLabel>,
    ) -> Task<R> {
        let dispatcher = self.dispatcher.clone();
        let (runnable, task) =
            async_task::spawn(future, move |runnable| dispatcher.dispatch(runnable, label));
        runnable.schedule();
        Task(TaskState::BackgroundSpawned(task))
    }

    /// Used by the test harness to run an async test in a synchronous fashion.
    #[cfg(any(test, feature = "test-support"))]
    #[track_caller]
    pub fn block_test<R>(&self, future: impl Future<Output = R>) -> R {
        if let Ok(value) = self.block_internal(false, future, None) {
            value
        } else {
            unreachable!()
        }
    }

    /// Block the current thread until the given future resolves.
    /// Consider using `block_with_timeout` instead.
    pub fn block<R>(&self, future: impl Future<Output = R>) -> R {
        if let Ok(value) = self.block_internal(true, future, None) {
            value
        } else {
            unreachable!()
        }
    }

    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn block_internal<R>(
        &self,
        _background_only: bool,
        future: impl Future<Output = R>,
        timeout: Option<Duration>,
    ) -> Result<R, impl Future<Output = R>> {
        use std::time::Instant;

        let mut future = Box::pin(future);
        if timeout == Some(Duration::ZERO) {
            return Err(future);
        }
        let deadline = timeout.map(|timeout| Instant::now() + timeout);

        let unparker = self.dispatcher.unparker();
        let waker = waker_fn(move || {
            unparker.unpark();
        });
        let mut cx = std::task::Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return Ok(result),
                Poll::Pending => {
                    let timeout =
                        deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
                    if !self.dispatcher.park(timeout)
                        && deadline.is_some_and(|deadline| deadline < Instant::now())
                    {
                        return Err(future);
                    }
                }
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[track_caller]
    pub(crate) fn block_internal<R>(
        &self,
        background_only: bool,
        future: impl Future<Output = R>,
        timeout: Option<Duration>,
    ) -> Result<R, impl Future<Output = R>> {
        use std::sync::atomic::AtomicBool;

        let mut future = Box::pin(future);
        if timeout == Some(Duration::ZERO) {
            return Err(future);
        }
        let Some(dispatcher) = self.dispatcher.as_test() else {
            return Err(future);
        };

        let mut max_ticks = if timeout.is_some() {
            dispatcher.gen_block_on_ticks()
        } else {
            usize::MAX
        };
        let unparker = self.dispatcher.unparker();
        let awoken = Arc::new(AtomicBool::new(false));
        let waker = waker_fn({
            let awoken = awoken.clone();
            move || {
                awoken.store(true, SeqCst);
                unparker.unpark();
            }
        });
        let mut cx = std::task::Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return Ok(result),
                Poll::Pending => {
                    if max_ticks == 0 {
                        return Err(future);
                    }
                    max_ticks -= 1;

                    if !dispatcher.tick(background_only) {
                        if awoken.swap(false, SeqCst) {
                            continue;
                        }

                        if !dispatcher.parking_allowed() {
                            let mut backtrace_message = String::new();
                            let mut waiting_message = String::new();
                            if let Some(backtrace) = dispatcher.waiting_backtrace() {
                                backtrace_message =
                                    format!("\nbacktrace of waiting future:\n{:?}", backtrace);
                            }
                            if let Some(waiting_hint) = dispatcher.waiting_hint() {
                                waiting_message = format!("\n  waiting on: {}\n", waiting_hint);
                            }
                            panic!(
                                    "parked with nothing left to run{waiting_message}{backtrace_message}",
                                )
                        }
                        self.dispatcher.park(None);
                    }
                }
            }
        }
    }

    /// Block the current thread until the given future resolves
    /// or `duration` has elapsed.
    pub fn block_with_timeout<R>(
        &self,
        duration: Duration,
        future: impl Future<Output = R>,
    ) -> Result<R, impl Future<Output = R>> {
        self.block_internal(true, future, Some(duration))
    }

    /// Scoped lets you start a number of tasks and waits
    /// for all of them to complete before returning.
    pub async fn scoped<'scope, F>(&self, scheduler: F)
    where
        F: FnOnce(&mut Scope<'scope>),
    {
        let mut scope = Scope::new(self.clone());
        (scheduler)(&mut scope);
        let spawned = mem::take(&mut scope.futures)
            .into_iter()
            .map(|f| self.spawn(f))
            .collect::<Vec<_>>();
        for task in spawned {
            task.await;
        }
    }

    /// Get the current time.
    ///
    /// Calling this instead of `std::time::Instant::now` allows the use
    /// of fake timers in tests.
    pub fn now(&self) -> Instant {
        self.dispatcher.now()
    }

    /// Returns a task that will complete after the given duration.
    /// Depending on other concurrent tasks the elapsed duration may be longer
    /// than requested.
    pub fn timer(&self, duration: Duration) -> Task<()> {
        if duration.is_zero() {
            return Task::ready(());
        }
        let (runnable, task) = async_task::spawn(async move {}, {
            let dispatcher = self.dispatcher.clone();
            move |runnable| dispatcher.dispatch_after(duration, runnable)
        });
        runnable.schedule();
        Task(TaskState::BackgroundSpawned(task))
    }

    /// in tests, start_waiting lets you indicate which task is waiting (for debugging only)
    #[cfg(any(test, feature = "test-support"))]
    pub fn start_waiting(&self) {
        self.dispatcher.as_test().unwrap().start_waiting();
    }

    /// in tests, removes the debugging data added by start_waiting
    #[cfg(any(test, feature = "test-support"))]
    pub fn finish_waiting(&self) {
        self.dispatcher.as_test().unwrap().finish_waiting();
    }

    /// in tests, run an arbitrary number of tasks (determined by the SEED environment variable)
    #[cfg(any(test, feature = "test-support"))]
    pub fn simulate_random_delay(&self) -> impl Future<Output = ()> {
        self.dispatcher.as_test().unwrap().simulate_random_delay()
    }

    /// in tests, indicate that a given task from `spawn_labeled` should run after everything else
    #[cfg(any(test, feature = "test-support"))]
    pub fn deprioritize(&self, task_label: TaskLabel) {
        self.dispatcher.as_test().unwrap().deprioritize(task_label)
    }

    /// in tests, move time forward. This does not run any tasks, but does make `timer`s ready.
    #[cfg(any(test, feature = "test-support"))]
    pub fn advance_clock(&self, duration: Duration) {
        self.dispatcher.as_test().unwrap().advance_clock(duration)
    }

    /// in tests, run one task.
    #[cfg(any(test, feature = "test-support"))]
    pub fn tick(&self) -> bool {
        self.dispatcher.as_test().unwrap().tick(false)
    }

    /// in tests, run all tasks that are ready to run. If after doing so
    /// the test still has outstanding tasks, this will panic. (See also `allow_parking`)
    #[cfg(any(test, feature = "test-support"))]
    pub fn run_until_parked(&self) {
        self.dispatcher.as_test().unwrap().run_until_parked()
    }

    /// in tests, prevents `run_until_parked` from panicking if there are outstanding tasks.
    /// This is useful when you are integrating other (non-GPUI) futures, like disk access, that
    /// do take real async time to run.
    #[cfg(any(test, feature = "test-support"))]
    pub fn allow_parking(&self) {
        self.dispatcher.as_test().unwrap().allow_parking();
    }

    /// undoes the effect of [`allow_parking`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn forbid_parking(&self) {
        self.dispatcher.as_test().unwrap().forbid_parking();
    }

    /// adds detail to the "parked with nothing let to run" message.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_waiting_hint(&self, msg: Option<String>) {
        self.dispatcher.as_test().unwrap().set_waiting_hint(msg);
    }

    /// in tests, returns the rng used by the dispatcher and seeded by the `SEED` environment variable
    #[cfg(any(test, feature = "test-support"))]
    pub fn rng(&self) -> StdRng {
        self.dispatcher.as_test().unwrap().rng()
    }

    /// How many CPUs are available to the dispatcher.
    pub fn num_cpus(&self) -> usize {
        #[cfg(any(test, feature = "test-support"))]
        return 4;

        #[cfg(not(any(test, feature = "test-support")))]
        return num_cpus::get();
    }

    /// Whether we're on the main thread.
    pub fn is_main_thread(&self) -> bool {
        self.dispatcher.is_main_thread()
    }

    #[cfg(any(test, feature = "test-support"))]
    /// in tests, control the number of ticks that `block_with_timeout` will run before timing out.
    pub fn set_block_on_ticks(&self, range: std::ops::RangeInclusive<usize>) {
        self.dispatcher.as_test().unwrap().set_block_on_ticks(range);
    }
}

/// ForegroundExecutor runs things on the main thread.
impl ForegroundExecutor {
    /// Creates a new ForegroundExecutor from the given PlatformDispatcher.
    pub fn new(dispatcher: Arc<dyn PlatformDispatcher>) -> Self {
        Self {
            dispatcher,
            _not_clone: NotClone,
            not_send: PhantomData,
        }
    }

    /// Cloning executors can cause runtime panics, see the documentation on `NotClone` for details.
    /// Use this power wisely.
    #[doc(hidden)]
    pub fn clone(&self) -> Self {
        Self {
            dispatcher: self.dispatcher.clone(),
            _not_clone: NotClone,
            not_send: PhantomData,
        }
    }

    /// Enqueues the given Task to run on the main thread at some point in the future.
    #[track_caller]
    pub fn spawn<R>(&self, future: impl Future<Output = R> + 'static) -> Task<R>
    where
        R: 'static,
    {
        let mut context = ForegroundContext::none();
        let task = self.spawn_internal(future, context);
        Task(TaskState::ForegroundSpawned(task))
    }

    /// Enqueues the given Task to run on the main thread at some point in the future,
    /// with a context parameter that will be checked before each turn
    #[track_caller]
    pub(crate) fn spawn_with_context<R>(
        &self,
        mut context: ForegroundContext,
        future: impl Future<Output = R> + 'static,
    ) -> Task<R>
    where
        R: 'static,
    {
        let task = self.spawn_internal(future, context);
        Task(TaskState::ForegroundSpawned(task))
    }

    #[track_caller]
    fn spawn_internal<R>(
        &self,
        future: impl Future<Output = R> + 'static,
        mut context: ForegroundContext,
    ) -> smol::Task<R, ForegroundContext>
    where
        R: 'static,
    {
        /// Declarations here are copy-modified from:
        /// https://github.com/smol-rs/async-task/blob/ca9dbe1db9c422fd765847fa91306e30a6bb58a9/src/runnable.rs#L405
        #[inline]
        pub(crate) fn thread_id() -> ThreadId {
            std::thread_local! {
                static ID: ThreadId = std::thread::current().id();
            }
            ID.try_with(|id| *id)
                .unwrap_or_else(|_| std::thread::current().id())
        }

        struct Checked<F> {
            id: ThreadId,
            location: core::panic::Location<'static>,
            inner: ManuallyDrop<F>,
        }

        impl<F> Drop for Checked<F> {
            fn drop(&mut self) {
                assert!(
                    self.id == thread_id(),
                    "local task dropped by a thread that didn't spawn it. Task spawned at {}",
                    self.location
                );
                unsafe {
                    ManuallyDrop::drop(&mut self.inner);
                }
            }
        }

        impl<F: Future> Future for Checked<F> {
            type Output = F::Output;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                assert!(
                    self.id == thread_id(),
                    "local task polled by a thread that didn't spawn it. Task spawned at {}",
                    self.location
                );
                unsafe { self.map_unchecked_mut(|c| &mut *c.inner).poll(cx) }
            }
        }

        let checked = Checked {
            id: thread_id(),
            location: *core::panic::Location::caller(),
            inner: ManuallyDrop::new(future),
        };

        let dispatcher = self.dispatcher.clone();
        let (runnable, task) = Builder::new().metadata(context).spawn_local(
            |_| checked,
            move |runnable| dispatcher.dispatch_on_main_thread(runnable),
        );
        runnable.schedule();
        task
    }
}

/// Scope manages a set of tasks that are enqueued and waited on together. See [`BackgroundExecutor::scoped`].
pub struct Scope<'a> {
    executor: BackgroundExecutor,
    futures: Vec<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>,
    tx: Option<mpsc::Sender<()>>,
    rx: mpsc::Receiver<()>,
    lifetime: PhantomData<&'a ()>,
}

impl<'a> Scope<'a> {
    fn new(executor: BackgroundExecutor) -> Self {
        let (tx, rx) = mpsc::channel(1);
        Self {
            executor,
            tx: Some(tx),
            rx,
            futures: Default::default(),
            lifetime: PhantomData,
        }
    }

    /// How many CPUs are available to the dispatcher.
    pub fn num_cpus(&self) -> usize {
        self.executor.num_cpus()
    }

    /// Spawn a future into this scope.
    pub fn spawn<F>(&mut self, f: F)
    where
        F: Future<Output = ()> + Send + 'a,
    {
        let tx = self.tx.clone().unwrap();

        // SAFETY: The 'a lifetime is guaranteed to outlive any of these futures because
        // dropping this `Scope` blocks until all of the futures have resolved.
        let f = unsafe {
            mem::transmute::<
                Pin<Box<dyn Future<Output = ()> + Send + 'a>>,
                Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
            >(Box::pin(async move {
                f.await;
                drop(tx);
            }))
        };
        self.futures.push(f);
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.tx.take().unwrap();

        // Wait until the channel is closed, which means that all of the spawned
        // futures have resolved.
        self.executor.block(self.rx.next());
    }
}
