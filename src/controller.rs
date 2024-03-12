use std::{
    sync::{
        atomic::{AtomicI8, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use crate::{
    hub::Hub,
    supervisor::Supervisor,
    thread_rt::{Builder, RTParams, Scheduling},
    DataDeliveryPolicy, Error,
};
use parking_lot::Mutex;
pub use roboplc_derive::WorkerOpts;

pub mod prelude {
    pub use super::{Context, Controller, Worker, WorkerOptions};
    pub use roboplc_derive::WorkerOpts;
}

const SLEEP_SLEEP: Duration = Duration::from_millis(100);

/// Controller state beacon. Can be cloned and shared with no limitations.
#[derive(Clone)]
pub struct State {
    state: Arc<AtomicI8>,
}

impl State {
    pub fn new() -> Self {
        Self {
            state: AtomicI8::new(StateKind::Starting as i8).into(),
        }
    }
    /// Set controller state
    pub fn set(&self, state: StateKind) {
        self.state.store(state as i8, Ordering::SeqCst);
    }
    /// Get controller state
    pub fn get(&self) -> StateKind {
        StateKind::from(self.state.load(Ordering::SeqCst))
    }
    /// Is the controller online (starting or running)
    pub fn is_online(&self) -> bool {
        self.get() >= StateKind::Starting
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// Controller state kind
#[derive(Default, Eq, PartialEq, Clone, Copy, Ord, PartialOrd)]
#[repr(i8)]
pub enum StateKind {
    #[default]
    Starting = 0,
    Active = 1,
    Running = 2,
    Stopping = -1,
    Stopped = -100,
    Unknown = -128,
}

impl From<i8> for StateKind {
    fn from(v: i8) -> Self {
        match v {
            0 => StateKind::Starting,
            1 => StateKind::Active,
            2 => StateKind::Running,
            -100 => StateKind::Stopped,
            _ => StateKind::Unknown,
        }
    }
}

/// Controller, used to manage workers and their context
///
/// Generic parameter `D` is the message type for the controller's [`Hub`] messages.
/// Generic parameter `V` is the type of shared variables. If shared variables are not required, it
/// can be set to `()`.
///
pub struct Controller<D, V>
where
    D: DataDeliveryPolicy + Clone + Send + Sync + 'static,
    V: Send + 'static,
{
    supervisor: Supervisor<()>,
    hub: Hub<D>,
    state: State,
    variables: Arc<Mutex<V>>,
}

impl<D, V> Controller<D, V>
where
    D: DataDeliveryPolicy + Clone + Send + Sync + 'static,
    V: Send + 'static,
{
    /// Creates a new controller instance, variables MUST implement [`Default`] trait
    pub fn new() -> Self
    where
        V: Default,
    {
        Self {
            supervisor: <_>::default(),
            hub: <_>::default(),
            state: State::new(),
            variables: <_>::default(),
        }
    }
    /// Creates a new controller instance with a pre-defined variables object
    pub fn new_with_variables(variables: V) -> Self
    where
        V: Default,
    {
        Self {
            supervisor: <_>::default(),
            hub: <_>::default(),
            state: State::new(),
            variables: Arc::new(Mutex::new(variables)),
        }
    }
    /// Spawns a worker
    pub fn spawn_worker<W: Worker<D, V> + WorkerOptions + 'static>(
        &mut self,
        mut worker: W,
    ) -> Result<(), Error> {
        let context = self.context();
        let mut rt_params = RTParams::new().set_scheduling(worker.worker_scheduling());
        if let Some(priority) = worker.worker_priority() {
            rt_params = rt_params.set_priority(priority);
        }
        if let Some(cpu_ids) = worker.worker_cpu_ids() {
            rt_params = rt_params.set_cpu_ids(cpu_ids);
        }
        let mut builder = Builder::new()
            .name(worker.worker_name())
            .rt_params(rt_params);
        if let Some(stack_size) = worker.worker_stack_size() {
            builder = builder.stack_size(stack_size);
        }
        self.supervisor.spawn(builder, move || {
            worker.run(&context);
        })?;
        Ok(())
    }
    /// Spawns a task thread (non-real-time)
    pub fn spawn_task<F>(&mut self, name: &str, f: F) -> Result<(), Error>
    where
        F: FnOnce() + Send + 'static,
    {
        self.supervisor.spawn(Builder::new().name(name), f)?;
        Ok(())
    }
    fn context(&self) -> Context<D, V> {
        Context {
            hub: self.hub.clone(),
            state: self.state.clone(),
            variables: self.variables.clone(),
        }
    }
    /// Blocks until all tasks/workers are finished
    pub fn block(&mut self) {
        self.supervisor.join_all();
        self.state.set(StateKind::Stopped);
    }
    /// Blocks until the controller goes into stopping/stopped
    pub fn block_while_online(&self) {
        while self.state.is_online() {
            thread::sleep(SLEEP_SLEEP);
        }
        self.state.set(StateKind::Stopped);
    }
    /// Is the controller online (starting or running)
    pub fn is_online(&self) {
        self.state.is_online();
    }
    /// State beacon
    pub fn state(&self) -> &State {
        &self.state
    }
    /// Controller [`Hub`] instance
    pub fn hub(&self) -> &Hub<D> {
        &self.hub
    }
    /// Controller [`Supervisor`] instance
    pub fn supervisor(&self) -> &Supervisor<()> {
        &self.supervisor
    }
    /// Controller shared variables
    pub fn variables(&self) -> &Arc<Mutex<V>> {
        &self.variables
    }
}

impl<D, V> Default for Controller<D, V>
where
    D: DataDeliveryPolicy + Clone + Send + Sync + 'static,
    V: Send + 'static + Default,
{
    fn default() -> Self {
        Self::new()
    }
}

/// The context type is used to give workers access to the controller's hub, state, and shared
/// variables.
pub struct Context<D, V>
where
    D: DataDeliveryPolicy + Clone + Send + Sync + 'static,
    V: Send,
{
    hub: Hub<D>,
    state: State,
    variables: Arc<Mutex<V>>,
}

impl<D, V> Context<D, V>
where
    D: DataDeliveryPolicy + Clone + Send + Sync + 'static,
    V: Send,
{
    /// Controller's hub instance
    pub fn hub(&self) -> &Hub<D> {
        &self.hub
    }
    /// Controller's shared variables (locked)
    pub fn variables(&self) -> &Arc<Mutex<V>> {
        &self.variables
    }
    /// Controller's state
    pub fn get_state(&self) -> StateKind {
        self.state.get()
    }
    /// Set controller's state
    pub fn set_state(&self, state: StateKind) {
        self.state.set(state);
    }
    /// Is the controller online (starting or running)
    pub fn is_online(&self) -> bool {
        self.state.is_online()
    }
}

/// The trait which MUST be implemented by all workers
pub trait Worker<D: DataDeliveryPolicy + Clone + Send + Sync + 'static, V: Send>:
    Send + Sync
{
    /// The worker's main function, started by [`Controller::spawn_worker()`]
    fn run(&mut self, context: &Context<D, V>);
}

/// The trait which MUST be implemented by all workers
pub trait WorkerOptions {
    /// A mandatory method, an unique name for the worker
    fn worker_name(&self) -> &str;
    /// The stack size for the worker thread
    fn worker_stack_size(&self) -> Option<usize> {
        None
    }
    /// The [`Scheduling`] policy for the worker thread
    fn worker_scheduling(&self) -> Scheduling {
        Scheduling::default()
    }
    /// The scheduled priority for the worker thread
    fn worker_priority(&self) -> Option<i32> {
        None
    }
    /// The CPU ID(s) affinity for the worker thread
    fn worker_cpu_ids(&self) -> Option<&[usize]> {
        None
    }
}