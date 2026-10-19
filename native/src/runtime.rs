use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rquickjs::{Context, Runtime};
use serde_json::Value;

// Wall-clock budget for one script load, action, or state write. QuickJS polls the interrupt
// handler while running bytecode, so a runaway loop stops once the deadline has passed.
#[cfg(not(test))]
const TIME_LIMIT: Duration = Duration::from_millis(2000);
#[cfg(test)]
const TIME_LIMIT: Duration = Duration::from_millis(1000);

pub struct JsBackend {
    // Context drops before Runtime (struct fields drop in declaration order).
    context: Context,
    _runtime: Runtime,
    // Set only while a call is running; the interrupt handler compares against it.
    deadline: Arc<Mutex<Option<Instant>>>,
    // The state the script started with. A hot reload compares it with `state` to tell values
    // the user changed from values the script still owns.
    initial: Value,
    state: Value,
}

impl JsBackend {
    pub fn new(script: &str) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        // Limit JS heap use in this prototype.
        runtime.set_memory_limit(32 * 1024 * 1024);
        runtime.set_max_stack_size(1024 * 1024);
        let deadline = Arc::new(Mutex::new(None));
        let armed = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || is_past(&armed))));
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        let mut backend = Self {
            context,
            _runtime: runtime,
            deadline,
            initial: Value::Null,
            state: Value::Null,
        };
        let state = backend.run_limited("Backend startup", |context| {
            context.with(|ctx| {
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
    /// `previous` is left alone, so a failed reload keeps the running app.
    pub fn reload(script: &str, previous: &JsBackend) -> Result<Self, String> {
        let mut backend = Self::new(script)?;
        backend.state = merge_preserved(&backend.state, &previous.initial, &previous.state);
        backend.restore_js_state();
        Ok(backend)
    }

    pub fn state(&self) -> &Value {
        &self.state
    }

    pub fn set_state(&mut self, key: &str, value: Value) -> Result<(), String> {
        let key = serde_json::to_string(key).map_err(|e| e.to_string())?;
        let value = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        // Failures come back as `{ ok: false, error }` so the host sees the real message.
        let source = format!(
            r#"(function() {{
                try {{
                    const app=globalThis.__tsgpui_app;
                    const key={key}, value={value}, parts=key.split('.');
                    let target=app.state;
                    for (const part of parts.slice(0,-1)) {{
                        if (Array.isArray(target)) {{
                            if (!/^\d+$/.test(part) || !Object.prototype.hasOwnProperty.call(target,Number(part))) throw Error('Unknown state key: '+key);
                            target=target[Number(part)];
                        }} else {{
                            if (!target || typeof target !== 'object' || !Object.prototype.hasOwnProperty.call(target,part)) throw Error('Unknown state key: '+key);
                            target=target[part];
                        }}
                    }}
                    const finalKey=parts[parts.length-1];
                    if (Array.isArray(target)) {{
                        if (!/^\d+$/.test(finalKey) || !Object.prototype.hasOwnProperty.call(target,Number(finalKey))) throw Error('Unknown state key: '+key);
                        target[Number(finalKey)]=value;
                    }} else {{
                        if (!target || typeof target !== 'object' || !Object.prototype.hasOwnProperty.call(target,finalKey)) throw Error('Unknown state key: '+key);
                        target[finalKey]=value;
                    }}
                    return JSON.stringify({{ok:true,state:JSON.stringify(app.state)}});
                }} catch (err) {{
                    return JSON.stringify({{ok:false,error:String((err && err.message) || err)}});
                }}
            }})()"#
        );
        let result = self.run_limited("Backend state update", |context| {
            let envelope = context
                .with(|ctx| ctx.eval::<String, _>(source.into_bytes()))
                .map_err(|e| format!("Backend state update failed: {e}"))?;
            take_state(&envelope, "Backend state update failed")
        });
        self.commit_or_restore(result)
    }

    pub fn invoke_with_event(&mut self, action: &str, event: Value) -> Result<(), String> {
        let name = serde_json::to_string(action).map_err(|e| e.to_string())?;
        let event = serde_json::to_string(&event).map_err(|e| e.to_string())?;
        // No ambient filesystem, network, process or Bun APIs are installed. Promise
        // actions may use QuickJS microtasks; host capabilities are explicit UI events.
        // Any failure restores the state captured before the action ran.
        let source = format!(
            r#"(function() {{
                const app=globalThis.__tsgpui_app;
                const message=(err) => String((err && err.message) || err);
                const fn=app.actions[{name}];
                if (typeof fn !== 'function') return Promise.resolve(JSON.stringify({{ok:false,error:'No action: ' + {name}}}));
                const before=JSON.stringify(app.state);
                const restore=(err) => {{ app.state=JSON.parse(before); return JSON.stringify({{ok:false,error:message(err)}}); }};
                const api=Object.freeze({{
                    get: (key) => app.state[key],
                    set: (key,value) => {{ if (!Object.prototype.hasOwnProperty.call(app.state,key)) throw Error('Unknown state key: '+key); app.state[key]=value; }},
                    update: (key,modify) => {{ if (!Object.prototype.hasOwnProperty.call(app.state,key)) throw Error('Unknown state key: '+key); app.state[key]=modify(app.state[key]); }}
                }});
                let pending;
                try {{
                    pending=Promise.resolve(fn(api,Object.freeze({event})));
                }} catch (err) {{
                    return Promise.resolve(restore(err));
                }}
                return pending.then(() => {{
                    const after=JSON.stringify(app.state);
                    if (typeof after !== 'string') throw Error('State is not JSON serializable');
                    return JSON.stringify({{ok:true,state:after}});
                }}).catch(restore);
            }})().then((envelope) => {{ globalThis.__tsgpui_envelope=envelope; }})"#
        );
        let armed = self.deadline.clone();
        let result = self
            .run_limited(&format!("Backend action '{action}'"), |context| {
                context.with(|ctx| {
                    ctx.eval::<(), _>("globalThis.__tsgpui_envelope = undefined;".as_bytes().to_vec())
                        .map_err(|e| format!("Backend action '{action}' failed: {e}"))?;
                    ctx.eval::<rquickjs::Value, _>(source.into_bytes())
                        .map_err(|e| format!("Backend action '{action}' failed: {e}"))?;
                    // The envelope is read back from a global once the job queue drains.
                    // eval_promise cannot be used: it does not flatten a promise returned by the script.
                    while ctx.execute_pending_job() && !is_past(&armed) {}
                    ctx.eval::<Option<String>, _>("globalThis.__tsgpui_envelope")
                        .map_err(|e| format!("Backend action '{action}' failed: {e}"))?
                        .ok_or_else(|| format!("Backend action '{action}' did not finish; it may be waiting on a promise that never settles"))
                })
            })
            .and_then(|envelope| take_state(&envelope, &format!("Backend action '{action}' failed")));
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
                spinAfterWrite(api) { api.set('count', 99); while (true) {} },
                async spinInJob(api) { await null; while (true) {} },
                async hang(api) { api.set('count', 99); await new Promise(() => {}); },
            }
        };
    "#;

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
