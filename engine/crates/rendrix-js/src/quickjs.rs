//! QuickJS backend for [`JsEngine`]. This is the only file that names `rquickjs`.

use crate::engine_trait::{Host, JsEngine, JsError, Val};
use rquickjs::function::{Args, Rest};
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{Array, Ctx, Error, Exception, Function, Module, Result as QResult, Runtime, Type, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type ModuleMap = Rc<RefCell<HashMap<String, String>>>;

struct MapResolver(ModuleMap);
struct MapLoader(ModuleMap);

/// Resolve `name` imported from `base` against the module map.
pub fn resolve_module(map: &HashMap<String, String>, base: &str, name: &str) -> Option<String> {
    let joined = if name.starts_with("./") || name.starts_with("../") {
        let dir = base.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let mut parts: Vec<&str> = if dir.is_empty() { vec![] } else { dir.split('/').collect() };
        for seg in name.split('/') {
            match seg {
                "." | "" => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        parts.join("/")
    } else {
        name.trim_start_matches('/').to_string()
    };
    [joined.clone(), format!("{joined}.js"), format!("{joined}/index.js"), format!("{joined}.mjs")].into_iter().find(|c| map.contains_key(c))
}

impl Resolver for MapResolver {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, base: &str, name: &str, _attrs: Option<ImportAttributes<'js>>) -> QResult<String> {
        resolve_module(&self.0.borrow(), base, name).ok_or_else(|| Error::new_resolving(base, name))
    }
}

impl Loader for MapLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str, _attrs: Option<ImportAttributes<'js>>) -> QResult<Module<'js, Declared>> {
        let src = self.0.borrow().get(name).cloned().ok_or_else(|| Error::new_loading(name))?;
        Module::declare(ctx.clone(), name, src)
    }
}

pub struct QuickJsEngine {
    rt: Runtime,
    ctx: rquickjs::Context,
    modules: ModuleMap,
    rejections: Rc<RefCell<Vec<String>>>,
}

fn to_val(v: &Value<'_>) -> Val {
    match v.type_of() {
        Type::Undefined | Type::Uninitialized => Val::Undefined,
        Type::Null => Val::Null,
        Type::Bool => Val::Bool(v.as_bool().unwrap_or(false)),
        Type::Int => Val::Num(v.as_int().unwrap_or(0) as f64),
        Type::Float => Val::Num(v.as_float().unwrap_or(0.0)),
        Type::String => Val::Str(v.as_string().and_then(|s| s.to_string().ok()).unwrap_or_default()),
        Type::Array => {
            let a = v.as_array().unwrap();
            Val::List((0..a.len()).map(|i| a.get::<Value>(i).map(|x| to_val(&x)).unwrap_or(Val::Undefined)).collect())
        }
        _ => Val::Undefined,
    }
}

fn from_val<'js>(ctx: &Ctx<'js>, v: &Val) -> QResult<Value<'js>> {
    Ok(match v {
        Val::Undefined => Value::new_undefined(ctx.clone()),
        Val::Null => Value::new_null(ctx.clone()),
        Val::Bool(b) => Value::new_bool(ctx.clone(), *b),
        Val::Num(n) => Value::new_number(ctx.clone(), *n),
        Val::Str(s) => rquickjs::String::from_str(ctx.clone(), s)?.into_value(),
        Val::List(items) => {
            let arr = Array::new(ctx.clone())?;
            for (i, x) in items.iter().enumerate() {
                arr.set(i, from_val(ctx, x)?)?;
            }
            arr.into_value()
        }
    })
}

fn describe_exception<'js>(ctx: &Ctx<'js>, err: Error) -> JsError {
    match err {
        Error::Exception => {
            let v = ctx.catch();
            if let Some(ex) = v.as_exception() {
                JsError { message: ex.message().unwrap_or_else(|| "Error".into()), stack: ex.stack() }
            } else if let Some(s) = v.as_string() {
                JsError { message: s.to_string().unwrap_or_default(), stack: None }
            } else {
                JsError { message: format!("uncaught exception: {:?}", v.type_of()), stack: None }
            }
        }
        other => JsError { message: other.to_string(), stack: None },
    }
}

fn install_native<'js>(ctx: &Ctx<'js>, host: Rc<RefCell<dyn Host>>) -> QResult<()> {
    let f = Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> QResult<Value<'js>> {
        let mut it = args.0.iter();
        let op = it.next().and_then(|v| v.as_string()).and_then(|s| s.to_string().ok()).unwrap_or_default();
        let vals: Vec<Val> = it.map(to_val).collect();
        let result = host.borrow_mut().call(&op, &vals);
        match result {
            Ok(v) => from_val(&ctx, &v),
            Err(msg) => Err(Exception::throw_message(&ctx, &msg)),
        }
    })?;
    ctx.globals().set("__native", f)?;
    Ok(())
}

impl QuickJsEngine {
    pub fn new(host: Rc<RefCell<dyn Host>>) -> Result<QuickJsEngine, JsError> {
        let rt = Runtime::new().map_err(|e| JsError { message: e.to_string(), stack: None })?;
        rt.set_memory_limit(1024 * 1024 * 1024);
        rt.set_max_stack_size(2 * 1024 * 1024);
        let modules: ModuleMap = Rc::new(RefCell::new(HashMap::new()));
        rt.set_loader(MapResolver(modules.clone()), MapLoader(modules.clone()));
        let rejections: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let r = rejections.clone();
            rt.set_host_promise_rejection_tracker(Some(Box::new(move |ctx, _promise, reason, handled| {
                if handled {
                    return;
                }
                let text = if let Some(ex) = reason.as_exception() {
                    format!("{}\n{}", ex.message().unwrap_or_default(), ex.stack().unwrap_or_default())
                } else if let Some(s) = reason.as_string() {
                    s.to_string().unwrap_or_default()
                } else {
                    let _ = ctx;
                    format!("{:?}", reason.type_of())
                };
                r.borrow_mut().push(text);
            })));
        }
        let ctx = rquickjs::Context::full(&rt).map_err(|e| JsError { message: e.to_string(), stack: None })?;
        ctx.with(|ctx| install_native(&ctx, host).map_err(|e| describe_exception(&ctx, e)))?;
        Ok(QuickJsEngine { rt, ctx, modules, rejections })
    }

}

impl JsEngine for QuickJsEngine {
    fn name(&self) -> &'static str {
        "quickjs"
    }

    fn add_module(&mut self, name: &str, source: String) {
        self.modules.borrow_mut().insert(name.to_string(), source);
    }

    fn eval_script(&mut self, name: &str, source: &str) -> Result<(), JsError> {
        self.ctx.with(|ctx| {
            let mut opts = rquickjs::context::EvalOptions::default();
            opts.global = true;
            opts.strict = false;
            ctx.eval_with_options::<(), _>(source.as_bytes().to_vec(), opts).map_err(|e| {
                let mut err = describe_exception(&ctx, e);
                err.message = format!("{name}: {}", err.message);
                err
            })
        })
    }

    fn eval_module(&mut self, name: &str) -> Result<(), JsError> {
        let src = self.modules.borrow().get(name).cloned().ok_or_else(|| JsError { message: format!("module '{name}' not found"), stack: None })?;
        self.ctx.with(|ctx| {
            let promise = Module::evaluate(ctx.clone(), name, src).map_err(|e| describe_exception(&ctx, e))?;
            match promise.finish::<()>() {
                Ok(()) => Ok(()),
                // Top-level await on something that needs the event loop: not an error.
                Err(Error::WouldBlock) => Ok(()),
                Err(e) => Err(describe_exception(&ctx, e)),
            }
        })
    }

    fn call_global(&mut self, func: &str, args: Vec<Val>) -> Result<Val, JsError> {
        self.ctx.with(|ctx| {
            let f: Function = ctx.globals().get(func).map_err(|e| describe_exception(&ctx, e))?;
            let mut a = Args::new(ctx.clone(), args.len());
            for v in &args {
                let jv = from_val(&ctx, v).map_err(|e| describe_exception(&ctx, e))?;
                a.push_arg(jv).map_err(|e| describe_exception(&ctx, e))?;
            }
            let out: Value = f.call_arg(a).map_err(|e| describe_exception(&ctx, e))?;
            Ok(to_val(&out))
        })
    }

    fn run_jobs(&mut self) -> Result<(), JsError> {
        loop {
            match self.rt.execute_pending_job() {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(_) => {
                    return self.ctx.with(|ctx| Err(describe_exception(&ctx, Error::Exception)));
                }
            }
        }
    }

    fn memory_usage(&self) -> usize {
        self.rt.memory_usage().memory_used_size as usize
    }

    fn collect_garbage(&mut self) {
        self.rt.run_gc();
    }

    fn take_rejections(&mut self) -> Vec<String> {
        std::mem::take(&mut *self.rejections.borrow_mut())
    }
}
