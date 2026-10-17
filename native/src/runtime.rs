use rquickjs::{Context,Runtime};
use serde_json::Value;

pub struct JsBackend {
    // Context drops before Runtime (struct fields drop in declaration order).
    context: Context,
    _runtime: Runtime,
    state: Value,
}

impl JsBackend {
    pub fn new(script: &str) -> Result<Self,String> {
        let runtime=Runtime::new().map_err(|e| e.to_string())?;
        // Limit JS heap use in this prototype. Timeouts/CPU interrupts are future work.
        runtime.set_memory_limit(32*1024*1024);
        runtime.set_max_stack_size(1024*1024);
        let context=Context::full(&runtime).map_err(|e| e.to_string())?;
        let state=context.with(|ctx| {
            ctx.eval::<(),_>(script.as_bytes().to_vec()).map_err(|e|format!("Backend evaluation failed: {e}"))?;
            let json: String = ctx.eval("JSON.stringify(globalThis.__tsgpui_app.state)")
                .map_err(|e|format!("Backend state serialization failed: {e}"))?;
            serde_json::from_str::<Value>(&json).map_err(|e|e.to_string())
        })?;
        Ok(Self{context,_runtime:runtime,state})
    }

    pub fn state(&self) -> &Value {&self.state}

    pub fn invoke(&mut self, action: &str) -> Result<(),String> {
        let name=serde_json::to_string(action).map_err(|e|e.to_string())?;
        // This is the *only* bridge: no ambient filesystem, network, process or Bun APIs.
        let source=format!(r#"(function() {{
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
                fn(api);
                const after=JSON.stringify(app.state);
                if (typeof after !== 'string') throw Error('State is not JSON serializable');
                return after;
            }} catch (err) {{
                app.state=JSON.parse(before);
                throw err;
            }}
        }})()"#);
        let json=self.context.with(|ctx| ctx.eval::<String,_>(source.into_bytes())
            .map_err(|e|format!("Backend action '{action}' failed: {e}")))?;
        let value: Value=serde_json::from_str(&json).map_err(|e|e.to_string())?;
        self.state=value;
        Ok(())
    }
}
