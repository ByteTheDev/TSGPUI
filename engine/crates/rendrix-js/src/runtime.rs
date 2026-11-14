//! The JavaScript runtime: prelude installation, module loading, and the task/event loop glue.

use crate::engine_trait::{JsEngine, JsError, Val};
use crate::host::{base64_encode, DomHost, Permissions, WindowCommand};
use crate::quickjs::QuickJsEngine;
use rendrix_core::Engine;
use rendrix_dom::NodeId;
use std::cell::RefCell;
use std::rc::Rc;

const EVENTS_JS: &str = include_str!("prelude/events.js");
const DOM_JS: &str = include_str!("prelude/dom.js");
const WEB_JS: &str = include_str!("prelude/web.js");
const JSX_RUNTIME_JS: &str = include_str!("prelude/jsx-runtime.js");

/// What the platform layer should do next.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TickInfo {
    /// `requestAnimationFrame` callbacks are waiting for the next frame.
    pub wants_frame: bool,
    /// Milliseconds (runtime clock) when the next timer is due.
    pub next_timer_ms: Option<f64>,
    /// A fetch is in flight (poll again soon).
    pub pending_io: bool,
}

pub struct JsRuntime {
    pub engine: Rc<RefCell<Engine>>,
    pub host: Rc<RefCell<DomHost>>,
    js: Box<dyn JsEngine>,
    /// Uncaught errors and diagnostics collected since the last `take_errors`.
    errors: Vec<String>,
}

impl JsRuntime {
    pub fn new(engine: Rc<RefCell<Engine>>, app_name: &str, permissions: Permissions) -> Result<JsRuntime, JsError> {
        let host = Rc::new(RefCell::new(DomHost::new(engine.clone(), app_name, permissions)));
        let mut js: Box<dyn JsEngine> = Box::new(QuickJsEngine::new(host.clone())?);
        let prelude = format!("(function(global){{'use strict';\nconst N = global.__native;\ndelete global.__native;\n{EVENTS_JS}\n{DOM_JS}\n{WEB_JS}\n}})(globalThis);");
        js.eval_script("rendrix:prelude", &prelude)?;
        js.add_module("rendrix", "export default globalThis.rendrix; export const { version, quit, window, fs, clipboard } = globalThis.rendrix;".to_string());
        js.add_module("rendrix/jsx-runtime", JSX_RUNTIME_JS.to_string());
        js.add_module("rendrix/jsx-dev-runtime", JSX_RUNTIME_JS.to_string());
        Ok(JsRuntime { engine, host, js, errors: Vec::new() })
    }

    pub fn engine_name(&self) -> &'static str {
        self.js.name()
    }

    pub fn add_module(&mut self, name: &str, source: String) {
        self.js.add_module(name, source);
    }

    pub fn memory_usage(&self) -> usize {
        self.js.memory_usage()
    }

    pub fn collect_garbage(&mut self) {
        self.js.collect_garbage();
    }

    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    fn record(&mut self, context: &str, e: JsError) {
        let text = format!("{context}: {e}");
        self.host.borrow_mut().call_console("error", &text);
        self.errors.push(text);
    }

    /// After every task: drain microtasks, deliver mutation records, report unhandled rejections.
    fn finish_task(&mut self) {
        if let Err(e) = self.js.run_jobs() {
            self.record("microtask", e);
        }
        if let Err(e) = self.js.call_global("__rendrix_afterTask", vec![]) {
            self.record("afterTask", e);
        }
        if let Err(e) = self.js.run_jobs() {
            self.record("microtask", e);
        }
        for r in self.js.take_rejections() {
            if let Err(e) = self.js.call_global("__rendrix_unhandledRejection", vec![Val::Str(r.clone())]) {
                self.record("unhandledrejection", e);
            }
            let _ = self.js.run_jobs();
        }
    }

    /// Run a classic script.
    pub fn run_script(&mut self, name: &str, source: &str) -> Result<(), JsError> {
        let r = self.js.eval_script(name, source);
        self.finish_task();
        r
    }

    /// Evaluate an ES module (and its imports) registered under `name`.
    pub fn run_module(&mut self, name: &str) -> Result<(), JsError> {
        let r = self.js.eval_module(name);
        self.finish_task();
        r
    }

    /// Fire DOMContentLoaded / load once all startup scripts have run.
    pub fn boot(&mut self) {
        if let Err(e) = self.js.call_global("__rendrix_boot", vec![]) {
            self.record("boot", e);
        }
        self.finish_task();
    }

    fn call(&mut self, func: &str, args: Vec<Val>) -> Val {
        match self.js.call_global(func, args) {
            Ok(v) => v,
            Err(e) => {
                self.record(func, e);
                Val::Undefined
            }
        }
    }

    /// Dispatch a trusted event. Returns `true` if no listener called `preventDefault`.
    pub fn dispatch(&mut self, event_type: &str, target: Option<NodeId>, init: &serde_json::Value) -> bool {
        let init_json = if init.is_null() { Val::Undefined } else { Val::Str(init.to_string()) };
        let target = target.map(|t| Val::Num(t as f64)).unwrap_or(Val::Num(-1.0));
        let r = self.call("__rendrix_dispatch", vec![Val::Str(event_type.to_string()), target, init_json]);
        self.finish_task();
        !matches!(r, Val::Bool(false))
    }

    /// Move focus (with blur/focus events). `None` clears focus.
    pub fn set_focus(&mut self, id: Option<NodeId>) {
        self.call("__rendrix_focus", vec![id.map(|i| Val::Num(i as f64)).unwrap_or(Val::Null)]);
        self.finish_task();
    }

    /// Fire an `input` event on a control after the platform edited its value.
    pub fn fire_input(&mut self, id: NodeId, input_type: &str, data: Option<&str>) {
        self.call("__rendrix_fireInput", vec![Val::Num(id as f64), Val::Str(input_type.into()), data.map(|d| Val::Str(d.into())).unwrap_or(Val::Null)]);
        self.finish_task();
    }

    /// Fire a `change` event (e.g. when a text field loses focus after editing).
    pub fn fire_change(&mut self, id: NodeId) {
        self.dispatch("change", Some(id), &serde_json::json!({ "bubbles": true }));
    }

    /// Implicit form submission (Enter in a single-line field).
    pub fn submit_form(&mut self, id: NodeId) {
        self.call("__rendrix_submitForm", vec![Val::Num(id as f64)]);
        self.finish_task();
    }

    pub fn viewport_changed(&mut self) {
        self.call("__rendrix_viewportChanged", vec![]);
        self.finish_task();
    }

    /// Run due timers and completed fetches. Call once per event-loop iteration.
    pub fn tick(&mut self) -> TickInfo {
        let now = self.host.borrow().now_ms();
        let due = self.host.borrow_mut().take_due_timers(now);
        for id in due {
            self.call("__rendrix_timer", vec![Val::Num(id as f64)]);
            self.finish_task();
        }
        let done = self.host.borrow_mut().poll_fetches();
        for f in done {
            let mut flat = Vec::new();
            for (k, v) in &f.headers {
                flat.push(Val::Str(k.clone()));
                flat.push(Val::Str(v.clone()));
            }
            self.call(
                "__rendrix_fetchDone",
                vec![Val::Num(f.id as f64), Val::Num(f.status as f64), Val::Str(f.status_text), Val::List(flat), Val::Str(base64_encode(&f.body)), f.error.map(Val::Str).unwrap_or(Val::Undefined)],
            );
            self.finish_task();
        }
        self.tick_info()
    }

    pub fn tick_info(&self) -> TickInfo {
        let h = self.host.borrow();
        TickInfo { wants_frame: h.raf_requested, next_timer_ms: h.next_timer_ms(), pending_io: h.pending_fetches > 0 || !h.immediate_fetches.is_empty() }
    }

    /// Run `requestAnimationFrame` callbacks for this frame (timestamp in ms).
    pub fn run_animation_frame(&mut self, timestamp_ms: f64) {
        self.host.borrow_mut().raf_requested = false;
        let more = self.call("__rendrix_raf", vec![Val::Num(timestamp_ms)]);
        if matches!(more, Val::Bool(true)) {
            self.host.borrow_mut().raf_requested = true;
        }
        self.finish_task();
    }

    /// Notify ResizeObservers after layout (runs inside the frame, after `Engine::update`).
    pub fn after_layout(&mut self) {
        self.call("__rendrix_checkResize", vec![]);
        self.finish_task();
    }

    pub fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        std::mem::take(&mut self.host.borrow_mut().window_commands)
    }

    pub fn quit_requested(&self) -> bool {
        self.host.borrow().quit_requested
    }

    /// Evaluate an expression and return its value (primitives and arrays of them), for tests and diagnostics.
    pub fn eval(&mut self, expr: &str) -> Result<Val, JsError> {
        let src = format!("globalThis.__rendrix_eval_get = function(){{ return ({expr}); }};");
        self.js.eval_script("eval", &src)?;
        let r = self.js.call_global("__rendrix_eval_get", vec![]);
        self.finish_task();
        r
    }
}

impl DomHost {
    pub fn call_console(&mut self, level: &str, msg: &str) {
        use crate::engine_trait::Host;
        let _ = self.call("console", &[Val::Str(level.into()), Val::Str(msg.into())]);
    }
}
