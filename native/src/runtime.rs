use rquickjs::{Context, Runtime};
use serde_json::Value;

pub struct JsBackend {
    // Context drops before Runtime (struct fields drop in declaration order).
    context: Context,
    _runtime: Runtime,
    state: Value,
}

impl JsBackend {
    pub fn new(script: &str) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        // Limit JS heap use in this prototype. CPU interrupts are not available yet.
        runtime.set_memory_limit(32 * 1024 * 1024);
        runtime.set_max_stack_size(1024 * 1024);
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        let state = context.with(|ctx| {
            ctx.eval::<(), _>(script.as_bytes().to_vec())
                .map_err(|e| format!("Backend evaluation failed: {e}"))?;
            let json: String = ctx
                .eval("JSON.stringify(globalThis.__tsgpui_app.state)")
                .map_err(|e| format!("Backend state serialization failed: {e}"))?;
            serde_json::from_str::<Value>(&json).map_err(|e| e.to_string())
        })?;
        Ok(Self {
            context,
            _runtime: runtime,
            state,
        })
    }

    pub fn state(&self) -> &Value {
        &self.state
    }

    pub fn set_state(&mut self, key: &str, value: Value) -> Result<(), String> {
        let key = serde_json::to_string(key).map_err(|e| e.to_string())?;
        let value = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        let source = format!(
            r#"(function() {{
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
                return JSON.stringify(app.state);
            }})()"#
        );
        let json = self
            .context
            .with(|ctx| ctx.eval::<String, _>(source.into_bytes()))
            .map_err(|e| format!("Backend state update failed: {e}"))?;
        self.state = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn invoke_with_event(&mut self, action: &str, event: Value) -> Result<(), String> {
        let name = serde_json::to_string(action).map_err(|e| e.to_string())?;
        let event = serde_json::to_string(&event).map_err(|e| e.to_string())?;
        // No ambient filesystem, network, process or Bun APIs are installed. Promise
        // actions may use QuickJS microtasks; host capabilities are explicit UI events.
        let source = format!(
            r#"(function() {{
                const app=globalThis.__tsgpui_app;
                const fn=app.actions[{name}];
                if (typeof fn !== 'function') throw Error('No action: ' + {name});
                const before=JSON.stringify(app.state);
                const api=Object.freeze({{
                    get: (key) => app.state[key],
                    set: (key,value) => {{ if (!Object.prototype.hasOwnProperty.call(app.state,key)) throw Error('Unknown state key: '+key); app.state[key]=value; }},
                    update: (key,modify) => {{ if (!Object.prototype.hasOwnProperty.call(app.state,key)) throw Error('Unknown state key: '+key); app.state[key]=modify(app.state[key]); }}
                }});
                try {{
                    return Promise.resolve(fn(api,Object.freeze({event}))).then(() => {{
                        const after=JSON.stringify(app.state);
                        if (typeof after !== 'string') throw Error('State is not JSON serializable');
                        return after;
                    }}).catch(err => {{ app.state=JSON.parse(before); throw err; }});
                }} catch (err) {{ app.state=JSON.parse(before); throw err; }}
            }})()"#
        );
        let json = self.context.with(|ctx| {
            let promise = ctx
                .eval_promise(source.into_bytes())
                .map_err(|e| format!("Backend action '{action}' failed: {e}"))?;
            promise
                .finish::<String>()
                .map_err(|e| format!("Backend action '{action}' failed: {e}"))
        })?;
        self.state = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        Ok(())
    }
}
