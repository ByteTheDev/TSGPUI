use std::{
    collections::{BTreeSet, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use rquickjs::{Context, Ctx, Function, Runtime};
use serde_json::{json, Value};

use crate::{
    capabilities::{text_arg, Capabilities},
    ipc::IpcHub,
};

// Wall-clock budget for one slice of backend JS: an action's first run, a continuation after a host
// result, a listener, or a state write. QuickJS polls the interrupt handler while running bytecode,
// so a runaway loop stops once the deadline has passed. Time spent waiting on the host does not count.
#[cfg(not(test))]
const TIME_LIMIT: Duration = Duration::from_millis(2000);
#[cfg(test)]
const TIME_LIMIT: Duration = Duration::from_millis(1000);

// Host operations run on this many threads, so a slow process or disk does not stall the others.
const WORKER_THREADS: usize = 4;

/// What the host threads send back to the runtime.
pub(crate) enum HostEvent {
    /// A host operation finished. `envelope` is the JSON `{ ok, value }` or `{ ok, error }`.
    Settled { id: u64, envelope: String },
    /// A message arrived for `channel`: from a peer, or from this app's own `ipc.send`.
    Inbound { channel: String, payload: Value },
}

/// A window change the backend asked for. The view applies these on the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub enum UiCommand {
    Minimize,
    Maximize,
    Restore,
    Close,
    SetTitle(String),
}

/// What one `pump` changed.
#[derive(Debug, Default)]
pub struct Pump {
    /// True when committed state, or the list of pending UI commands, may have changed.
    pub changed: bool,
    /// Failures from work that no direct caller is waiting for: queued actions, listeners, and
    /// continuations of finished actions.
    pub errors: Vec<String>,
}

// Work the runtime has accepted but not started. Work runs one task at a time, so each task starts
// from committed state and rolls back cleanly, even when it waits on the host.
enum Task {
    Action { name: String, event: Value },
    Deliver { channel: String, payload: Value },
    SetState { key: String, value: Value },
}

// The task that is waiting on host results. `start_id` is the first host request it may have issued.
struct Running {
    label: String,
    start_id: u64,
}

type Job = Box<dyn FnOnce() + Send>;

// Shared with the JS host function and with worker jobs.
#[derive(Clone)]
struct HostPort {
    capabilities: Arc<Capabilities>,
    ipc: Option<Arc<IpcHub>>,
    events: Sender<HostEvent>,
    jobs: Sender<Job>,
    next_id: Arc<AtomicU64>,
    // Requests whose result has not been delivered yet.
    unsettled: Arc<Mutex<BTreeSet<u64>>>,
    ui: Arc<Mutex<Vec<UiCommand>>>,
}

impl HostPort {
    fn issue(&self) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.unsettled.lock().unwrap_or_else(|error| error.into_inner()).insert(id);
        id
    }

    fn settle_now(&self, id: u64, error: impl Into<String>) {
        let envelope = json!({ "ok": false, "error": error.into() }).to_string();
        let _ = self.events.send(HostEvent::Settled { id, envelope });
    }

    // Queues `operation` for a worker thread. The result arrives later as a `Settled` event.
    fn submit(&self, id: u64, operation: String, args: Vec<Value>) {
        let port = self.clone();
        let job: Job = Box::new(move || {
            let envelope = match port.perform(&operation, &args) {
                Ok(value) => json!({ "ok": true, "value": value }),
                Err(error) => json!({ "ok": false, "error": error }),
            };
            let _ = port.events.send(HostEvent::Settled { id, envelope: envelope.to_string() });
        });
        let _ = self.jobs.send(job);
    }

    fn perform(&self, operation: &str, args: &[Value]) -> Result<Value, String> {
        if let Some(command) = operation.strip_prefix("window.") {
            if !self.capabilities.window {
                return Err("Window control is not granted: set permissions.window to true".into());
            }
            let command = match command {
                "minimize" => UiCommand::Minimize,
                "maximize" => UiCommand::Maximize,
                "restore" => UiCommand::Restore,
                "close" => UiCommand::Close,
                "setTitle" => UiCommand::SetTitle(text_arg(args, 0)?.to_string()),
                _ => return Err(format!("Native operation '{operation}' is not available in this build")),
            };
            self.ui.lock().unwrap_or_else(|error| error.into_inner()).push(command);
            return Ok(Value::Null);
        }
        if operation == "ipc.send" {
            let Some(hub) = self.ipc.as_ref() else {
                return Err("IPC is not granted: set permissions.ipc to true".into());
            };
            let channel = text_arg(args, 0)?;
            let payload = args.get(1).cloned().unwrap_or(Value::Null);
            hub.broadcast(channel, &payload);
            // The app's own listeners hear what it sends, the same as peers do.
            let _ = self.events.send(HostEvent::Inbound { channel: channel.to_string(), payload });
            return Ok(Value::Null);
        }
        let environment = self.ipc.as_ref().map(|hub| hub.environment()).unwrap_or_default();
        self.capabilities.call(operation, args, &environment)
    }
}

pub struct JsBackend {
    // Context drops before Runtime (struct fields drop in declaration order).
    context: Context,
    _runtime: Runtime,
    // Set only while a slice of JS runs; the interrupt handler compares against it.
    deadline: Arc<Mutex<Option<Instant>>>,
    // The state the script started with. A hot reload compares it with `state` to tell values
    // the user changed from values the script still owns.
    initial: Value,
    // Committed state. Only finished tasks change it, so a waiting action never shows partial writes.
    state: Value,
    // The grants from the bundle's permissions. Reloads keep them.
    capabilities: Arc<Capabilities>,
    host: HostPort,
    events: Receiver<HostEvent>,
    queue: VecDeque<Task>,
    running: Option<Running>,
}

impl JsBackend {
    /// Starts `script` with no host grants: every `api.native` call is refused.
    #[cfg(test)]
    pub fn new(script: &str) -> Result<Self, String> {
        Self::with_capabilities(script, Capabilities::default())
    }

    /// Starts `script` with the host operations that `capabilities` grants.
    pub fn with_capabilities(script: &str, capabilities: Capabilities) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        // Limit JS heap use in this prototype.
        runtime.set_memory_limit(32 * 1024 * 1024);
        runtime.set_max_stack_size(1024 * 1024);
        let deadline = Arc::new(Mutex::new(None));
        let armed = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || is_past(&armed))));
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;

        let (events_sender, events) = mpsc::channel();
        let ipc = if capabilities.ipc {
            Some(IpcHub::start(events_sender.clone()).map_err(|e| format!("Unable to start IPC: {e}"))?)
        } else {
            None
        };
        let grants = json!({
            "window": capabilities.window,
            "notifications": capabilities.notifications,
            "ipc": capabilities.ipc,
        })
        .to_string();
        let capabilities = Arc::new(capabilities);
        let host = HostPort {
            capabilities: capabilities.clone(),
            ipc,
            events: events_sender,
            jobs: start_workers(),
            next_id: Arc::new(AtomicU64::new(1)),
            unsettled: Arc::new(Mutex::new(BTreeSet::new())),
            ui: Arc::new(Mutex::new(Vec::new())),
        };
        let mut backend = Self {
            context,
            _runtime: runtime,
            deadline: deadline.clone(),
            initial: Value::Null,
            state: Value::Null,
            capabilities,
            host: host.clone(),
            events,
            queue: VecDeque::new(),
            running: None,
        };
        let state = backend.run_limited("Backend startup", |context| {
            context.with(|ctx| {
                install_host(&ctx, host).map_err(|e| format!("Backend startup failed: {e}"))?;
                ctx.eval::<(), _>(NATIVE_SETUP.replace("__TSGPUI_GRANTS__", &grants).into_bytes())
                    .map_err(|e| format!("Backend startup failed: {e}"))?;
                ctx.eval::<(), _>(script.as_bytes().to_vec())
                    .map_err(|e| format!("Backend evaluation failed: {e}"))?;
                let json: String = ctx
                    .eval("JSON.stringify(globalThis.__tsgpui_app.state)")
                    .map_err(|e| format!("Backend state serialization failed: {e}"))?;
                serde_json::from_str::<Value>(&json).map_err(|e| e.to_string())
            })
        })?;
        backend.initial = state.clone();
        backend.state = state;
        Ok(backend)
    }

    /// Starts `script` again and carries over the runtime state of `previous`. See [`merge_preserved`].
    /// `previous` is left alone, so a failed reload keeps the running app. Host calls that `previous`
    /// still has in flight finish, but their results go nowhere.
    pub fn reload(script: &str, previous: &JsBackend) -> Result<Self, String> {
        let mut backend = Self::with_capabilities(script, previous.capabilities.as_ref().clone())?;
        backend.state = merge_preserved(&backend.state, &previous.initial, &previous.state);
        backend.restore_js_state();
        Ok(backend)
    }

    pub fn state(&self) -> &Value {
        &self.state
    }

    /// True while a task waits on host results, or while tasks are queued behind it.
    pub fn is_busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    /// Applies state saved by a previous run. Values the user changed survive, keys the current
    /// script no longer declares are dropped, and values whose kind changed take the new default.
    pub fn merge_saved(&mut self, saved: Value) {
        self.state = merge_preserved(&self.state, &self.initial, &saved);
        self.restore_js_state();
    }

    /// Sets one state key, such as `user.name`. While a task is waiting, the write waits its turn.
    pub fn set_state(&mut self, key: &str, value: Value) -> Result<(), String> {
        if self.is_busy() {
            self.queue.push_back(Task::SetState { key: key.to_string(), value });
            return Ok(());
        }
        self.apply_set_state(key, value)
    }

    /// Runs `action`. Returns the action's error if it failed without waiting on the host. If it
    /// waits, this returns Ok and the outcome arrives through [`JsBackend::pump`].
    pub fn invoke_with_event(&mut self, action: &str, event: Value) -> Result<(), String> {
        let task = Task::Action { name: action.to_string(), event };
        if self.is_busy() {
            self.queue.push_back(task);
            return Ok(());
        }
        self.begin(task).map(|_| ())
    }

    /// Delivers finished host results and starts queued work. Call this regularly while the app runs.
    pub fn pump(&mut self) -> Pump {
        let mut pump = Pump::default();
        let mut settled = false;
        while let Ok(event) = self.events.try_recv() {
            pump.changed = true;
            match event {
                HostEvent::Settled { id, envelope } => {
                    self.host.unsettled.lock().unwrap_or_else(|error| error.into_inner()).remove(&id);
                    settled = true;
                    let source = format!("globalThis.__tsgpui_settle({id}, {})", literal(&envelope));
                    if let Err(error) = self.slice("Host call", Some(source)) {
                        pump.errors.push(error);
                        // With no task waiting, nothing else will roll back this partial work.
                        // A waiting task rolls back when it concludes.
                        if self.running.is_none() {
                            self.restore_js_state();
                        }
                    }
                }
                HostEvent::Inbound { channel, payload } => {
                    self.queue.push_back(Task::Deliver { channel, payload });
                }
            }
        }
        if settled && self.running.is_none() {
            self.commit_background();
        }
        self.advance(&mut pump);
        pump
    }

    /// Takes the window changes the backend has asked for since the last call.
    pub fn take_ui_commands(&mut self) -> Vec<UiCommand> {
        std::mem::take(&mut *self.host.ui.lock().unwrap_or_else(|error| error.into_inner()))
    }

    /// Where other local processes can reach this app's IPC hub, as environment variables. Empty
    /// unless the `ipc` permission is granted.
    #[cfg(test)]
    pub fn ipc_environment(&self) -> Vec<(String, String)> {
        self.host.ipc.as_ref().map(|hub| hub.environment()).unwrap_or_default()
    }

    // Starts a task now. Ok(true) means it finished and committed. Ok(false) means it is waiting on
    // the host, and `running` is set. Err means it failed and its state was rolled back.
    fn begin(&mut self, task: Task) -> Result<bool, String> {
        let (label, source) = match task {
            Task::SetState { key, value } => return self.apply_set_state(&key, value).map(|()| true),
            Task::Action { name, event } => {
                let source = format!("globalThis.__tsgpui_envelope = undefined;\n{}", action_source(&name, &event));
                (format!("Backend action '{name}'"), source)
            }
            Task::Deliver { channel, payload } => {
                let source = format!("globalThis.__tsgpui_envelope = undefined;\n{}", deliver_source(&channel, &payload));
                (format!("Backend listener for '{channel}'"), source)
            }
        };
        let start_id = self.host.next_id.load(Ordering::SeqCst);
        let slice = self.slice(&label, Some(source));
        self.conclude(&label, start_id, slice)
    }

    // Decides what one slice of JS work means for the task that owns it. `slice` is the envelope the
    // task has produced so far, if any.
    fn conclude(&mut self, label: &str, start_id: u64, slice: Result<Option<String>, String>) -> Result<bool, String> {
        match slice {
            Ok(Some(envelope)) => {
                self.running = None;
                let result = take_state(&envelope, &format!("{label} failed"));
                self.commit_or_restore(result).map(|()| true)
            }
            Ok(None) if self.has_unsettled_since(start_id) => {
                self.running = Some(Running { label: label.to_string(), start_id });
                Ok(false)
            }
            Ok(None) => {
                self.running = None;
                self.restore_js_state();
                Err(format!("{label} did not finish; it may be waiting on a promise that never settles"))
            }
            Err(error) => {
                self.running = None;
                self.restore_js_state();
                Err(error)
            }
        }
    }

    // Starts queued tasks while nothing is waiting, and checks whether the waiting task has finished.
    fn advance(&mut self, pump: &mut Pump) {
        loop {
            if let Some(Running { label, start_id }) = self.running.take() {
                // `conclude` puts `running` back if the task is still waiting.
                let slice = self.slice(&label, None);
                match self.conclude(&label, start_id, slice) {
                    Ok(true) => pump.changed = true,
                    Ok(false) => return,
                    Err(error) => {
                        pump.changed = true;
                        pump.errors.push(error);
                    }
                }
                continue;
            }
            let Some(task) = self.queue.pop_front() else { return };
            pump.changed = true;
            if let Err(error) = self.begin(task) {
                pump.errors.push(error);
            }
        }
    }

    // Runs `source` (if any) and the jobs it queues, then reads the task envelope if one was produced.
    fn slice(&self, label: &str, source: Option<String>) -> Result<Option<String>, String> {
        self.run_limited(label, |context| {
            context.with(|ctx| {
                if let Some(source) = source {
                    ctx.eval::<rquickjs::Value, _>(source.into_bytes())
                        .map_err(|e| format!("{label} failed: {e}"))?;
                }
                // The envelope is read back from a global once the job queue drains.
                // eval_promise cannot be used: it does not flatten a promise returned by the script.
                while ctx.execute_pending_job() && !is_past(&self.deadline) {}
                // A job cut short by the interrupt handler is a time-limit failure, not a stalled task.
                if is_past(&self.deadline) {
                    return Err(format!("{label} was interrupted"));
                }
                ctx.eval::<Option<String>, _>("globalThis.__tsgpui_envelope")
                    .map_err(|e| format!("{label} failed: {e}"))
            })
        })
    }

    fn has_unsettled_since(&self, start_id: u64) -> bool {
        self.host.unsettled.lock().unwrap_or_else(|error| error.into_inner()).range(start_id..).next().is_some()
    }

    // Work that continues after its action has finished, such as a host result that resolved an
    // un-awaited promise, changes state with nothing to roll back. Commit it as it happens.
    fn commit_background(&mut self) {
        let Ok(json) = self.context.with(|ctx| {
            ctx.eval::<String, _>("JSON.stringify(globalThis.__tsgpui_app.state)")
        }) else {
            return;
        };
        if let Ok(state) = serde_json::from_str::<Value>(&json) {
            self.state = state;
        }
    }

    fn apply_set_state(&mut self, key: &str, value: Value) -> Result<(), String> {
        // Failures come back as `{ ok: false, error }` so the host sees the real message.
        let source = SET_STATE_SOURCE
            .replace("__KEY__", &literal(key))
            .replace("__VALUE__", &value.to_string());
        let result = self.run_limited("Backend state update", |context| {
            let envelope = context
                .with(|ctx| ctx.eval::<String, _>(source.into_bytes()))
                .map_err(|e| format!("Backend state update failed: {e}"))?;
            take_state(&envelope, "Backend state update failed")
        });
        self.commit_or_restore(result)
    }

    // Runs `work` with the time limit armed. A call that ran out of time is reported as such,
    // whatever error the interrupted work surfaced.
    fn run_limited<T>(
        &self,
        what: &str,
        work: impl FnOnce(&Context) -> Result<T, String>,
    ) -> Result<T, String> {
        self.set_deadline(Some(Instant::now() + TIME_LIMIT));
        let result = work(&self.context);
        let timed_out = is_past(&self.deadline);
        self.set_deadline(None);
        match result {
            Err(_) if timed_out => Err(format!("{what} exceeded the {} ms time limit", TIME_LIMIT.as_millis())),
            other => other,
        }
    }

    fn set_deadline(&self, value: Option<Instant>) {
        *self.deadline.lock().unwrap_or_else(|e| e.into_inner()) = value;
    }

    // Commits a successful result. On failure the JS heap is reset to the last committed state,
    // because an interrupted action cannot run its own rollback.
    fn commit_or_restore(&mut self, result: Result<Value, String>) -> Result<(), String> {
        match result {
            Ok(state) => {
                self.state = state;
                Ok(())
            }
            Err(error) => {
                self.restore_js_state();
                Err(error)
            }
        }
    }

    fn restore_js_state(&self) {
        let literal = serde_json::to_string(&self.state.to_string()).unwrap_or_else(|_| "null".into());
        let source = format!("globalThis.__tsgpui_app.state = JSON.parse({literal});");
        let _ = self.context.with(|ctx| ctx.eval::<(), _>(source.into_bytes()));
    }
}

/// Three-way merge for hot reload. `seed` is the state the previous script started with, `current`
/// is the state the app holds now, and `fresh` is the state the reloaded script starts with.
/// A value the user changed (current differs from seed) is kept when the new script has the same
/// kind of value. A value that still equals its seed takes the new seed. Keys the new script no
/// longer has are dropped, and new keys take their seed. Arrays the user changed are kept whole.
pub fn merge_preserved(fresh: &Value, seed: &Value, current: &Value) -> Value {
    if seed == current {
        return fresh.clone();
    }
    match (fresh, current) {
        (Value::Object(fresh_map), Value::Object(_)) => Value::Object(
            fresh_map
                .iter()
                .map(|(key, fresh_value)| {
                    let merged = match current.get(key) {
                        Some(current_value) => merge_preserved(fresh_value, seed.get(key).unwrap_or(&Value::Null), current_value),
                        None => fresh_value.clone(),
                    };
                    (key.clone(), merged)
                })
                .collect(),
        ),
        (Value::Array(_), Value::Array(_))
        | (Value::String(_), Value::String(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Null, Value::Null) => current.clone(),
        _ => fresh.clone(),
    }
}

fn is_past(deadline: &Mutex<Option<Instant>>) -> bool {
    match deadline.lock() {
        Ok(guard) => guard.is_some_and(|at| Instant::now() >= at),
        Err(_) => true,
    }
}

fn literal(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

// Starts the threads that run host operations. They exit once every sender is dropped.
fn start_workers() -> Sender<Job> {
    let (sender, receiver) = mpsc::channel::<Job>();
    let receiver = Arc::new(Mutex::new(receiver));
    for _ in 0..WORKER_THREADS {
        let receiver = receiver.clone();
        thread::spawn(move || loop {
            let job = receiver.lock().unwrap_or_else(|error| error.into_inner()).recv();
            match job {
                Ok(job) => job(),
                Err(_) => break,
            }
        });
    }
    sender
}

// Runs once before the app script. It wraps the host function in promise-returning methods and
// installs the settle hook the runtime uses to resume them. The methods are frozen and
// non-enumerable, and each call is checked against the grants in Rust. `__TSGPUI_GRANTS__` is
// replaced with the grant flags.
const NATIVE_SETUP: &str = r#"(function() {
    const host = globalThis.__tsgpui_host;
    Reflect.deleteProperty(globalThis, '__tsgpui_host');
    const grants = __TSGPUI_GRANTS__;
    const hidden = (name, value) => Object.defineProperty(globalThis, name, { value, enumerable: false, writable: false, configurable: false });
    // Each host call gets an id from Rust. Its promise waits here until the result is settled.
    const waiting = new Map();
    const call = (operation) => (...args) => new Promise((resolve, reject) => {
        const id = host(operation, JSON.stringify(args));
        waiting.set(id, { resolve, reject });
    });
    const settle = (id, envelopeJson) => {
        const entry = waiting.get(id);
        if (!entry) return;
        waiting.delete(id);
        const envelope = JSON.parse(envelopeJson);
        if (envelope.ok) entry.resolve(envelope.value);
        else entry.reject(Error(envelope.error));
    };
    const subscribers = new Map();
    const on = (channel, listener) => {
        if (!grants.ipc) throw Error('IPC is not granted: set permissions.ipc to true');
        if (typeof listener !== 'function') throw Error('IPC listener must be a function');
        const set = subscribers.get(channel) ?? new Set();
        subscribers.set(channel, set);
        set.add(listener);
        return () => { set.delete(listener); };
    };
    const dispatch = (channel) => Array.from(subscribers.get(channel) ?? []);
    const native = Object.freeze({
        window: Object.freeze({
            minimize: call('window.minimize'),
            maximize: call('window.maximize'),
            restore: call('window.restore'),
            close: call('window.close'),
            setTitle: (title) => call('window.setTitle')(title)
        }),
        filesystem: Object.freeze({
            readText: call('filesystem.readText'),
            writeText: call('filesystem.writeText'),
            readBytes: async (path) => new Uint8Array(await call('filesystem.readBytes')(path)),
            writeBytes: (path, bytes) => call('filesystem.writeBytes')(path, Array.from(bytes)),
            list: call('filesystem.list'),
            stat: call('filesystem.stat'),
            mkdir: call('filesystem.mkdir'),
            remove: call('filesystem.remove'),
            rename: call('filesystem.rename')
        }),
        process: Object.freeze({
            spawn: (command, args) => call('process.spawn')(command, args ?? [])
        }),
        notifications: Object.freeze({
            show: (options) => call('notifications.show')(options)
        }),
        ipc: Object.freeze({
            send: (channel, payload) => call('ipc.send')(channel, payload === undefined ? null : payload),
            on
        })
    });
    hidden('__tsgpui_native', native);
    hidden('__tsgpui_settle', settle);
    hidden('__tsgpui_dispatch', dispatch);
})();"#;

// Runs one action. `__NAME__` and `__EVENT__` are replaced with JSON literals. The event goes in last
// so user data is never scanned for placeholders.
const ACTION_SOURCE: &str = r#"(function() {
    const app=globalThis.__tsgpui_app;
    const message=(err) => String((err && err.message) || err);
    const fn=app.actions[__NAME__];
    if (typeof fn !== 'function') return Promise.resolve(JSON.stringify({ok:false,error:'No action: ' + __NAME__}));
    const before=JSON.stringify(app.state);
    const restore=(err) => { app.state=JSON.parse(before); return JSON.stringify({ok:false,error:message(err)}); };
    const api=Object.freeze({
        get: (key) => app.state[key],
        set: (key,value) => { if (!Object.prototype.hasOwnProperty.call(app.state,key)) throw Error('Unknown state key: '+key); app.state[key]=value; },
        update: (key,modify) => { if (!Object.prototype.hasOwnProperty.call(app.state,key)) throw Error('Unknown state key: '+key); app.state[key]=modify(app.state[key]); },
        native: globalThis.__tsgpui_native
    });
    let pending;
    try {
        pending=Promise.resolve(fn(api,Object.freeze(__EVENT__)));
    } catch (err) {
        return Promise.resolve(restore(err));
    }
    return pending.then(() => {
        const after=JSON.stringify(app.state);
        if (typeof after !== 'string') throw Error('State is not JSON serializable');
        return JSON.stringify({ok:true,state:after});
    }).catch(restore);
})().then((envelope) => { globalThis.__tsgpui_envelope=envelope; });"#;

// Calls every `ipc.on` listener for a channel as one task, so a listener that writes state rolls
// back when it fails. `__CHANNEL__` and `__PAYLOAD__` are JSON literals, with the payload last.
const DELIVER_SOURCE: &str = r#"(function() {
    const app=globalThis.__tsgpui_app;
    const message=(err) => String((err && err.message) || err);
    const before=JSON.stringify(app.state);
    const restore=(err) => { app.state=JSON.parse(before); return JSON.stringify({ok:false,error:message(err)}); };
    const listeners=globalThis.__tsgpui_dispatch(__CHANNEL__);
    return Promise.resolve()
        .then(async () => { for (const listener of listeners) { await listener(__PAYLOAD__); } })
        .then(() => JSON.stringify({ok:true,state:JSON.stringify(app.state)}), restore)
        .then((envelope) => { globalThis.__tsgpui_envelope=envelope; });
})();"#;

// Sets one key and returns `{ ok, state | error }`. The key path must already exist in the state.
const SET_STATE_SOURCE: &str = r#"(function() {
    try {
        const app=globalThis.__tsgpui_app;
        const key=__KEY__, value=__VALUE__, parts=key.split('.');
        let target=app.state;
        for (const part of parts.slice(0,-1)) {
            if (Array.isArray(target)) {
                if (!/^\d+$/.test(part) || !Object.prototype.hasOwnProperty.call(target,Number(part))) throw Error('Unknown state key: '+key);
                target=target[Number(part)];
            } else {
                if (!target || typeof target !== 'object' || !Object.prototype.hasOwnProperty.call(target,part)) throw Error('Unknown state key: '+key);
                target=target[part];
            }
        }
        const finalKey=parts[parts.length-1];
        if (Array.isArray(target)) {
            if (!/^\d+$/.test(finalKey) || !Object.prototype.hasOwnProperty.call(target,Number(finalKey))) throw Error('Unknown state key: '+key);
            target[Number(finalKey)]=value;
        } else {
            if (!target || typeof target !== 'object' || !Object.prototype.hasOwnProperty.call(target,finalKey)) throw Error('Unknown state key: '+key);
            target[finalKey]=value;
        }
        return JSON.stringify({ok:true,state:JSON.stringify(app.state)});
    } catch (err) {
        return JSON.stringify({ok:false,error:String((err && err.message) || err)});
    }
})()"#;

fn action_source(name: &str, event: &Value) -> String {
    ACTION_SOURCE.replace("__NAME__", &literal(name)).replace("__EVENT__", &event.to_string())
}

fn deliver_source(channel: &str, payload: &Value) -> String {
    DELIVER_SOURCE.replace("__CHANNEL__", &literal(channel)).replace("__PAYLOAD__", &payload.to_string())
}

// Exposes `__tsgpui_host(operation, argsJson)` to the setup script. It returns the request id at once,
// and the result arrives later through `HostEvent::Settled`.
fn install_host(ctx: &Ctx<'_>, port: HostPort) -> rquickjs::Result<()> {
    let host = Function::new(ctx.clone(), move |operation: String, args: String| -> f64 {
        let id = port.issue();
        match serde_json::from_str::<Value>(&args) {
            Ok(Value::Array(items)) => port.submit(id, operation, items),
            Ok(_) => port.settle_now(id, "Native arguments must be a JSON array"),
            Err(error) => port.settle_now(id, error.to_string()),
        }
        id as f64
    })?;
    ctx.globals().set("__tsgpui_host", host)
}

// Unwraps the `{ ok, state | error }` envelope produced by the JS helpers.
fn take_state(envelope: &str, prefix: &str) -> Result<Value, String> {
    let envelope: Value = serde_json::from_str(envelope).map_err(|e| e.to_string())?;
    if envelope["ok"] == Value::Bool(true) {
        let state = envelope["state"].as_str().ok_or("Backend returned no state")?;
        serde_json::from_str(state).map_err(|e| e.to_string())
    } else {
        Err(format!("{prefix}: {}", envelope["error"].as_str().unwrap_or("unknown error")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const APP: &str = r#"
        globalThis.__tsgpui_app = {
            state: { count: 0, last: null, user: { name: 'ada' }, items: [1, 2] },
            actions: {
                increment(api) { api.set('count', api.get('count') + 1); },
                record(api, event) { api.set('last', event.value); },
                update(api) { api.update('count', (n) => n + 10); },
                failAfterWrite(api) { api.set('count', 99); throw new Error('boom'); },
                async failAsync(api) { api.set('count', 99); await Promise.resolve(); throw new Error('late'); },
                writeUnknown(api) { api.set('missing', 1); },
                async asyncIncrement(api) { await Promise.resolve(); api.set('count', api.get('count') + 5); },
                async useNative(api) { await api.native.window.minimize(); },
                spinAfterWrite(api) { api.set('count', 99); while (true) {} },
                async spinInJob(api) { await null; while (true) {} },
                async hang(api) { api.set('count', 99); await new Promise(() => {}); },
            }
        };
    "#;

    // Waits until the runtime has no task in progress, and returns the failures it reported on the way.
    fn finish(backend: &mut JsBackend) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut errors = Vec::new();
        while backend.is_busy() {
            assert!(Instant::now() < deadline, "backend did not become idle");
            errors.extend(backend.pump().errors);
            if backend.is_busy() {
                thread::sleep(Duration::from_millis(2));
            }
        }
        errors
    }

    fn backend() -> JsBackend {
        JsBackend::new(APP).expect("test app should load")
    }

    #[test]
    fn loads_initial_state() {
        let backend = backend();
        assert_eq!(backend.state()["count"], json!(0));
        assert_eq!(backend.state()["user"]["name"], json!("ada"));
    }

    #[test]
    fn rejects_script_without_app_object() {
        assert!(JsBackend::new("var x = 1;").err().unwrap().contains("state serialization failed"));
    }

    #[test]
    fn rejects_script_with_syntax_error() {
        assert!(JsBackend::new("globalThis.__tsgpui_app = {").err().unwrap().contains("evaluation failed"));
    }

    #[test]
    fn rejects_startup_loop_that_never_finishes() {
        assert!(JsBackend::new("while (true) {}").err().unwrap().contains("time limit"));
    }

    #[test]
    fn set_state_updates_top_level_and_nested_keys() {
        let mut backend = backend();
        backend.set_state("count", json!(7)).unwrap();
        backend.set_state("user.name", json!("grace")).unwrap();
        backend.set_state("items.1", json!(5)).unwrap();
        assert_eq!(backend.state()["count"], json!(7));
        assert_eq!(backend.state()["user"]["name"], json!("grace"));
        assert_eq!(backend.state()["items"], json!([1, 5]));
    }

    #[test]
    fn set_state_rejects_unknown_keys_without_changing_state() {
        let mut backend = backend();
        assert!(backend.set_state("nope", json!(1)).unwrap_err().contains("Unknown state key: nope"));
        assert!(backend.set_state("user.age", json!(1)).unwrap_err().contains("Unknown state key"));
        assert!(backend.set_state("items.9", json!(1)).unwrap_err().contains("Unknown state key"));
        assert_eq!(backend.state()["user"], json!({ "name": "ada" }));
    }

    #[test]
    fn action_updates_state() {
        let mut backend = backend();
        backend.invoke_with_event("increment", json!({})).unwrap();
        backend.invoke_with_event("update", json!({})).unwrap();
        assert_eq!(backend.state()["count"], json!(11));
    }

    #[test]
    fn action_receives_frozen_event_payload() {
        let mut backend = backend();
        backend.invoke_with_event("record", json!({ "value": "typed" })).unwrap();
        assert_eq!(backend.state()["last"], json!("typed"));
    }

    #[test]
    fn async_action_updates_state_after_promise_settles() {
        let mut backend = backend();
        backend.invoke_with_event("asyncIncrement", json!({})).unwrap();
        assert_eq!(backend.state()["count"], json!(5));
    }

    #[test]
    fn unknown_action_is_an_error() {
        let mut backend = backend();
        assert!(backend.invoke_with_event("missing", json!({})).unwrap_err().contains("No action: missing"));
    }

    const FILE_APP: &str = r#"
        globalThis.__tsgpui_app = {
            state: { text: '', listed: 0 },
            actions: {
                async saveAndLoad(api) {
                    await api.native.filesystem.writeText('a.txt', 'hi');
                    api.set('text', await api.native.filesystem.readText('a.txt'));
                },
                async escape(api) {
                    api.set('text', 'changed');
                    await api.native.filesystem.writeText('../outside.txt', 'no');
                },
                async listData(api) {
                    api.set('listed', (await api.native.filesystem.list('')).length);
                },
                async bytes(api) {
                    await api.native.filesystem.writeBytes('b.bin', new Uint8Array([1, 2, 3]));
                    const back = await api.native.filesystem.readBytes('b.bin');
                    api.set('listed', back.length);
                }
            }
        };
    "#;

    fn granted_backend(name: &str) -> (JsBackend, std::path::PathBuf) {
        use crate::package::{FilesystemPermissions, Permissions};
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("tsgpui-runtime-{name}-{stamp}"));
        std::fs::create_dir_all(&base).unwrap();
        let permissions = Permissions {
            filesystem: FilesystemPermissions { read: vec!["data".into()], write: vec!["data".into()] },
            ..Permissions::default()
        };
        let grants = Capabilities::from_permissions(&permissions, base.clone());
        (JsBackend::with_capabilities(FILE_APP, grants).expect("file app should load"), base)
    }

    #[test]
    fn granted_filesystem_calls_work_from_an_action() {
        let (mut backend, _base) = granted_backend("roundtrip");
        backend.invoke_with_event("saveAndLoad", json!({})).unwrap();
        assert!(finish(&mut backend).is_empty());
        assert_eq!(backend.state()["text"], json!("hi"));
        backend.invoke_with_event("listData", json!({})).unwrap();
        assert!(finish(&mut backend).is_empty());
        assert_eq!(backend.state()["listed"], json!(1));
        backend.invoke_with_event("bytes", json!({})).unwrap();
        assert!(finish(&mut backend).is_empty());
        assert_eq!(backend.state()["listed"], json!(3));
    }

    #[test]
    fn denied_filesystem_call_rolls_back_the_action() {
        let (mut backend, base) = granted_backend("escape");
        backend.invoke_with_event("escape", json!({})).unwrap();
        let errors = finish(&mut backend);
        assert!(errors.iter().any(|error| error.contains("must not contain '..'")), "{errors:?}");
        assert_eq!(backend.state()["text"], json!(""));
        assert!(!base.join("outside.txt").exists());
    }

    #[test]
    fn filesystem_calls_are_refused_without_grants() {
        let mut backend = JsBackend::new(FILE_APP).unwrap();
        backend.invoke_with_event("saveAndLoad", json!({})).unwrap();
        let errors = finish(&mut backend);
        assert!(errors.iter().any(|error| error.contains("not inside a directory granted")), "{errors:?}");
        assert_eq!(backend.state()["text"], json!(""));
    }

    #[test]
    fn reload_keeps_the_filesystem_grants() {
        let (backend, _base) = granted_backend("reload");
        let mut reloaded = JsBackend::reload(FILE_APP, &backend).unwrap();
        reloaded.invoke_with_event("saveAndLoad", json!({})).unwrap();
        assert!(finish(&mut reloaded).is_empty());
        assert_eq!(reloaded.state()["text"], json!("hi"));
    }

    #[test]
    fn queued_actions_run_in_order_after_a_waiting_action() {
        let (mut backend, _base) = granted_backend("queue");
        backend.invoke_with_event("saveAndLoad", json!({})).unwrap();
        assert!(backend.is_busy(), "the host write is still in flight");
        backend.invoke_with_event("listData", json!({})).unwrap();
        backend.set_state("text", json!("manual")).unwrap();
        assert!(finish(&mut backend).is_empty());
        assert_eq!(backend.state()["listed"], json!(1), "listData saw the saved file");
        assert_eq!(backend.state()["text"], json!("manual"), "the queued write came last");
    }

    #[test]
    fn invoke_returns_while_a_host_call_is_still_running() {
        let (mut backend, _base) = granted_backend("nonblocking");
        let started = Instant::now();
        backend.invoke_with_event("bytes", json!({})).unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(finish(&mut backend).is_empty());
    }

    #[test]
    fn host_waits_do_not_count_against_the_time_limit() {
        let (name, args) = if cfg!(windows) {
            ("cmd", json!(["/C", "ping -n 3 127.0.0.1 >nul"]))
        } else {
            ("sh", json!(["-c", "sleep 2"]))
        };
        let base = std::env::temp_dir().join(format!("tsgpui-slow-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let permissions = crate::package::Permissions { processes: vec![name.into()], ..Default::default() };
        let grants = Capabilities::from_permissions(&permissions, base);
        let app = format!(
            "globalThis.__tsgpui_app = {{ state: {{ done: 0 }}, actions: {{ async slow(api) {{ \
                await api.native.process.spawn('{name}', {args}); api.set('done', 1); }} }} }};"
        );
        let mut backend = JsBackend::with_capabilities(&app, grants).unwrap();
        let started = Instant::now();
        backend.invoke_with_event("slow", json!({})).unwrap();
        assert!(started.elapsed() < Duration::from_millis(500), "invoke should not wait for the process");
        assert!(finish(&mut backend).is_empty());
        assert_eq!(backend.state()["done"], json!(1));
    }

    #[test]
    fn runaway_continuation_after_an_action_is_rolled_back() {
        use crate::package::{FilesystemPermissions, Permissions};
        let base = std::env::temp_dir().join(format!("tsgpui-background-{}", std::process::id()));
        std::fs::create_dir_all(base.join("data")).unwrap();
        let permissions = Permissions {
            filesystem: FilesystemPermissions { read: vec!["data".into()], write: vec![] },
            ..Permissions::default()
        };
        let grants = Capabilities::from_permissions(&permissions, base);
        // The action returns at once. The un-awaited continuation writes state, then loops forever.
        let app = "globalThis.__tsgpui_app = { state: { count: 0 }, actions: { fire(api) { \
            api.native.filesystem.list('').then(() => { api.set('count', 99); while (true) {} }); } } };";
        let mut backend = JsBackend::with_capabilities(app, grants).unwrap();
        backend.invoke_with_event("fire", json!({})).unwrap();
        let mut errors = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while errors.is_empty() {
            assert!(Instant::now() < deadline, "the continuation never reported");
            errors.extend(backend.pump().errors);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(errors[0].contains("time limit"), "{errors:?}");
        assert_eq!(backend.state()["count"], json!(0));
        backend.set_state("count", json!(2)).unwrap();
        assert_eq!(backend.state()["count"], json!(2));
    }

    #[test]
    fn window_calls_are_refused_without_the_window_grant() {
        let mut backend = backend();
        backend.invoke_with_event("useNative", json!({})).unwrap();
        let errors = finish(&mut backend);
        assert!(errors.iter().any(|error| error.contains("permissions.window")), "{errors:?}");
        assert_eq!(backend.state()["count"], json!(0));
    }

    #[test]
    fn window_commands_are_queued_for_the_ui_thread() {
        let permissions = crate::package::Permissions { window: true, ..Default::default() };
        let grants = Capabilities::from_permissions(&permissions, std::env::temp_dir());
        let app = "globalThis.__tsgpui_app = { state: { n: 0 }, actions: { async chrome(api) { \
            await api.native.window.setTitle('Notes'); await api.native.window.minimize(); } } };";
        let mut backend = JsBackend::with_capabilities(app, grants).unwrap();
        backend.invoke_with_event("chrome", json!({})).unwrap();
        assert!(finish(&mut backend).is_empty());
        assert_eq!(
            backend.take_ui_commands(),
            vec![UiCommand::SetTitle("Notes".into()), UiCommand::Minimize]
        );
        assert!(backend.take_ui_commands().is_empty());
    }

    #[test]
    fn ipc_messages_reach_local_listeners() {
        let permissions = crate::package::Permissions { ipc: true, ..Default::default() };
        let grants = Capabilities::from_permissions(&permissions, std::env::temp_dir());
        let app = "globalThis.__tsgpui_app = { state: { got: '' }, actions: {\
            listen(api) { api.native.ipc.on('loop', (payload) => api.set('got', payload)); },\
            async echo(api) { await api.native.ipc.send('loop', 'echo'); } } };";
        let mut backend = JsBackend::with_capabilities(app, grants).unwrap();
        backend.invoke_with_event("listen", json!({})).unwrap();
        backend.invoke_with_event("echo", json!({})).unwrap();
        assert!(finish(&mut backend).is_empty());
        assert_eq!(backend.state()["got"], json!("echo"));
    }

    #[test]
    fn ipc_peers_exchange_messages_with_the_app() {
        use std::{io::{BufRead, BufReader, Write}, net::TcpStream};
        let permissions = crate::package::Permissions { ipc: true, ..Default::default() };
        let grants = Capabilities::from_permissions(&permissions, std::env::temp_dir());
        let app = "globalThis.__tsgpui_app = { state: { got: '' }, actions: {\
            listen(api) { api.native.ipc.on('ping', (payload) => api.set('got', payload)); },\
            async say(api) { await api.native.ipc.send('pong', 'hello peer'); } } };";
        let mut backend = JsBackend::with_capabilities(app, grants).unwrap();
        backend.invoke_with_event("listen", json!({})).unwrap();
        let environment: std::collections::HashMap<String, String> = backend.ipc_environment().into_iter().collect();
        let mut peer = TcpStream::connect(&environment["TSGPUI_IPC_ADDR"]).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let token = &environment["TSGPUI_IPC_TOKEN"];
        writeln!(peer, "{}", json!({ "token": token })).unwrap();
        writeln!(peer, "{}", json!({ "channel": "ping", "payload": "from peer" })).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while backend.state()["got"] != json!("from peer") {
            assert!(Instant::now() < deadline, "the peer's message never arrived");
            backend.pump();
            thread::sleep(Duration::from_millis(5));
        }

        backend.invoke_with_event("say", json!({})).unwrap();
        assert!(finish(&mut backend).is_empty());
        let mut line = String::new();
        BufReader::new(peer).read_line(&mut line).unwrap();
        let message: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(message, json!({ "channel": "pong", "payload": "hello peer" }));
    }

    #[test]
    fn ipc_refuses_peers_without_the_token() {
        use std::{io::Write, net::TcpStream};
        let permissions = crate::package::Permissions { ipc: true, ..Default::default() };
        let grants = Capabilities::from_permissions(&permissions, std::env::temp_dir());
        let app = "globalThis.__tsgpui_app = { state: { got: '' }, actions: {\
            listen(api) { api.native.ipc.on('ping', (payload) => api.set('got', payload)); } } };";
        let mut backend = JsBackend::with_capabilities(app, grants).unwrap();
        backend.invoke_with_event("listen", json!({})).unwrap();
        let addr = backend.ipc_environment().into_iter().find(|(key, _)| key == "TSGPUI_IPC_ADDR").unwrap().1;
        let mut intruder = TcpStream::connect(addr).unwrap();
        writeln!(intruder, "{}", json!({ "token": "wrong" })).unwrap();
        writeln!(intruder, "{}", json!({ "channel": "ping", "payload": "spoof" })).unwrap();
        thread::sleep(Duration::from_millis(100));
        backend.pump();
        assert_eq!(backend.state()["got"], json!(""));
    }

    #[test]
    fn raw_host_function_is_hidden_from_app_code() {
        let mut backend = JsBackend::new(
            "globalThis.__tsgpui_app = { state: { kinds: [] }, actions: { probe(api) { \
                api.set('kinds', [typeof __tsgpui_host, typeof __tsgpui_native === 'object' ? 'native' : 'none']); } } };",
        ).unwrap();
        backend.invoke_with_event("probe", json!({})).unwrap();
        assert_eq!(backend.state()["kinds"], json!(["undefined", "native"]));
    }

    #[test]
    fn merge_saved_keeps_user_values_and_drops_stale_keys() {
        let mut backend = backend();
        backend.merge_saved(json!({ "count": 7, "user": { "name": "grace" }, "items": [9], "removed": true }));
        assert_eq!(backend.state()["count"], json!(7));
        assert_eq!(backend.state()["user"]["name"], json!("grace"));
        assert_eq!(backend.state()["items"], json!([9]));
        assert!(backend.state().get("removed").is_none());
        backend.invoke_with_event("increment", json!({})).unwrap();
        assert_eq!(backend.state()["count"], json!(8));
    }

    #[test]
    fn throwing_action_rolls_back_partial_writes() {
        let mut backend = backend();
        assert!(backend.invoke_with_event("failAfterWrite", json!({})).unwrap_err().contains("boom"));
        assert_eq!(backend.state()["count"], json!(0));
    }

    #[test]
    fn rejected_promise_action_rolls_back_partial_writes() {
        let mut backend = backend();
        assert!(backend.invoke_with_event("failAsync", json!({})).unwrap_err().contains("late"));
        assert_eq!(backend.state()["count"], json!(0));
    }

    #[test]
    fn action_writing_unknown_key_is_rejected_and_rolled_back() {
        let mut backend = backend();
        assert!(backend.invoke_with_event("writeUnknown", json!({})).unwrap_err().contains("Unknown state key: missing"));
        assert_eq!(backend.state()["count"], json!(0));
        assert!(backend.state().get("missing").is_none());
    }

    #[test]
    fn runaway_action_times_out_rolls_back_and_backend_stays_usable() {
        let mut backend = backend();
        let error = backend.invoke_with_event("spinAfterWrite", json!({})).unwrap_err();
        assert!(error.contains("Backend action 'spinAfterWrite' exceeded"), "{error}");
        assert_eq!(backend.state()["count"], json!(0));
        backend.invoke_with_event("increment", json!({})).unwrap();
        assert_eq!(backend.state()["count"], json!(1));
    }

    #[test]
    fn runaway_loop_after_await_times_out() {
        let mut backend = backend();
        let error = backend.invoke_with_event("spinInJob", json!({})).unwrap_err();
        assert!(error.contains("time limit"), "{error}");
        backend.invoke_with_event("increment", json!({})).unwrap();
    }

    #[test]
    fn action_that_never_settles_is_reported_and_rolled_back() {
        let mut backend = backend();
        let error = backend.invoke_with_event("hang", json!({})).unwrap_err();
        assert!(error.contains("did not finish"), "{error}");
        assert_eq!(backend.state()["count"], json!(0));
    }

    #[test]
    fn set_state_is_not_affected_by_an_earlier_timeout() {
        let mut backend = backend();
        let _ = backend.invoke_with_event("spinAfterWrite", json!({}));
        backend.set_state("count", json!(4)).unwrap();
        assert_eq!(backend.state()["count"], json!(4));
    }

    #[test]
    fn no_ambient_host_apis_are_exposed() {
        let mut backend = JsBackend::new(
            "globalThis.__tsgpui_app = { state: { kinds: [] }, actions: { probe(api) { \
                api.set('kinds', [typeof require, typeof process, typeof fetch, typeof Bun]); } } };",
        ).unwrap();
        backend.invoke_with_event("probe", json!({})).unwrap();
        assert_eq!(backend.state()["kinds"], json!(["undefined", "undefined", "undefined", "undefined"]));
    }

    #[test]
    fn merge_keeps_user_changes_and_takes_new_seeds() {
        let seed = json!({ "count": 0, "name": "ada", "items": [1] });
        let current = json!({ "count": 5, "name": "ada", "items": [1, 2] });
        let fresh = json!({ "count": 10, "name": "grace", "items": [3], "extra": true });
        assert_eq!(
            merge_preserved(&fresh, &seed, &current),
            json!({ "count": 5, "name": "grace", "items": [1, 2], "extra": true })
        );
    }

    #[test]
    fn merge_drops_keys_the_new_script_removed() {
        let merged = merge_preserved(&json!({ "a": 1 }), &json!({ "a": 0, "b": 0 }), &json!({ "a": 0, "b": 4 }));
        assert_eq!(merged, json!({ "a": 1 }));
    }

    #[test]
    fn merge_replaces_values_whose_kind_changed() {
        let merged = merge_preserved(&json!({ "v": "text" }), &json!({ "v": 0 }), &json!({ "v": 7 }));
        assert_eq!(merged, json!({ "v": "text" }));
    }

    #[test]
    fn merge_recurses_into_nested_objects() {
        let seed = json!({ "user": { "name": "ada", "age": 36 } });
        let current = json!({ "user": { "name": "ada", "age": 37 } });
        let fresh = json!({ "user": { "name": "grace", "age": 1 } });
        assert_eq!(merge_preserved(&fresh, &seed, &current), json!({ "user": { "name": "grace", "age": 37 } }));
    }

    #[test]
    fn reload_keeps_runtime_state_and_the_js_heap_agrees() {
        let mut old = backend();
        old.set_state("count", json!(7)).unwrap();
        let mut reloaded = JsBackend::reload(
            "globalThis.__tsgpui_app = { state: { count: 100, user: { name: 'grace' }, items: [9] }, actions: { \
                increment(api) { api.set('count', api.get('count') + 1); } } };",
            &old,
        ).unwrap();
        assert_eq!(reloaded.state()["count"], json!(7));
        assert_eq!(reloaded.state()["user"]["name"], json!("grace"));
        assert_eq!(reloaded.state()["items"], json!([9]));
        reloaded.invoke_with_event("increment", json!({})).unwrap();
        assert_eq!(reloaded.state()["count"], json!(8));
        assert_eq!(old.state()["count"], json!(7), "the previous backend is not modified");
    }

    #[test]
    fn failed_reload_leaves_the_previous_backend_usable() {
        let mut old = backend();
        old.set_state("count", json!(3)).unwrap();
        assert!(JsBackend::reload("globalThis.__tsgpui_app = {", &old).is_err());
        old.invoke_with_event("increment", json!({})).unwrap();
        assert_eq!(old.state()["count"], json!(4));
    }
}
