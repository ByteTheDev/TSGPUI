use crate::*;
use rendrix_core::{Engine, MapAssets};
use rendrix_dom::Document;
use std::cell::RefCell;
use std::rc::Rc;

fn rt_with(html: &str, assets: MapAssets) -> JsRuntime {
    let doc = Document::parse(html);
    let mut engine = Engine::with_document(doc, Box::new(assets));
    engine.set_viewport(400.0, 300.0, 1.0);
    let engine = Rc::new(RefCell::new(engine));
    let rt = JsRuntime::new(engine, "rendrix-test", Permissions::allow_all()).expect("runtime");
    rt.host.borrow_mut().echo_console = false;
    rt.host.borrow_mut().disable_storage_persistence();
    rt
}

fn rt(html: &str) -> JsRuntime {
    rt_with(html, MapAssets::default())
}

fn run(rt: &mut JsRuntime, src: &str) {
    if let Err(e) = rt.run_script("test", src) {
        panic!("script failed: {e}");
    }
}

fn s(rt: &mut JsRuntime, expr: &str) -> String {
    match rt.eval(&format!("(()=>{{ const v = ({expr}); return typeof v === 'string' ? v : JSON.stringify(v); }})()")) {
        Ok(Val::Str(s)) => s,
        Ok(other) => format!("{other:?}"),
        Err(e) => panic!("eval failed: {e}"),
    }
}

fn pump(rt: &mut JsRuntime, ms: u64) {
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        rt.tick();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    rt.tick();
}

fn logs(rt: &mut JsRuntime) -> Vec<String> {
    rt.host.borrow_mut().take_console().into_iter().map(|(l, m)| format!("{l}:{m}")).collect()
}

#[test]
fn engine_runs_modern_javascript() {
    let mut r = rt("");
    assert_eq!(s(&mut r, "[1,2,3].map(x => x ** 2).join(',')"), "1,4,9");
    run(&mut r, "class A { #p = 41; static s = 1; get v() { return this.#p + 1; } } globalThis.out = [new A().v, A.s, [...'ab'], `t${1+1}`, 1_000, 10n ** 3n === 1000n, a?.b ?? 'dflt'].toString(); var a;");
    assert_eq!(s(&mut r, "globalThis.out"), "42,1,a,b,t2,1000,true,dflt");
    assert_eq!(r.engine_name(), "quickjs");
}

#[test]
fn promises_microtasks_and_async_await_run_in_order() {
    let mut r = rt("");
    run(
        &mut r,
        "globalThis.order = []; const log = x => order.push(x);
         (async () => { log('a1'); await null; log('a2'); await new Promise(r => setTimeout(r, 5)); log('a3'); })();
         Promise.resolve().then(() => log('p1')); queueMicrotask(() => log('m1')); setTimeout(() => log('t0'), 0); log('sync');",
    );
    assert_eq!(s(&mut r, "order"), r#"["a1","sync","a2","p1","m1"]"#, "microtasks ran after the script, before timers");
    pump(&mut r, 40);
    assert_eq!(s(&mut r, "order"), r#"["a1","sync","a2","p1","m1","t0","a3"]"#);
}

#[test]
fn timers_intervals_and_clearing() {
    let mut r = rt("");
    run(&mut r, "globalThis.n = 0; globalThis.id = setInterval(() => { n++; if (n === 3) clearInterval(id); }, 5); const t = setTimeout(() => { n = 100; }, 5); clearTimeout(t);");
    pump(&mut r, 80);
    assert_eq!(s(&mut r, "n"), "3");
    assert!(r.tick_info().next_timer_ms.is_none(), "no timers left");
}

#[test]
fn request_animation_frame_runs_on_frame_not_on_tick() {
    let mut r = rt("");
    run(&mut r, "globalThis.frames = []; const loop = t => { frames.push(typeof t); if (frames.length < 3) requestAnimationFrame(loop); }; requestAnimationFrame(loop);");
    assert!(r.tick_info().wants_frame);
    pump(&mut r, 10);
    assert_eq!(s(&mut r, "frames.length"), "0", "rAF waits for the frame driver");
    r.run_animation_frame(16.0);
    assert!(r.tick_info().wants_frame, "callback re-requested a frame");
    r.run_animation_frame(32.0);
    r.run_animation_frame(48.0);
    assert_eq!(s(&mut r, "frames.join()"), "number,number,number");
    assert!(!r.tick_info().wants_frame);
}

#[test]
fn dom_creation_manipulation_and_queries() {
    let mut r = rt("<div id=app><ul class='list'><li class='a'>1</li><li>2</li></ul></div>");
    run(
        &mut r,
        "const ul = document.querySelector('ul'); const li = document.createElement('li'); li.textContent = 'three'; li.classList.add('x', 'y'); li.dataset.fooBar = '7'; ul.appendChild(li);
         globalThis.res = { n: ul.children.length, last: ul.lastElementChild.textContent, cls: li.className, data: li.getAttribute('data-foo-bar'), ds: li.dataset.fooBar,
           q: document.querySelectorAll('li.x').length, closest: li.closest('#app').id, matches: li.matches('ul > li:last-child'), same: ul.lastChild === li && document.querySelector('.y') === li,
           html: document.getElementById('app').innerHTML.length > 20, tag: li.tagName, name: li.nodeName, parent: li.parentNode === ul, doc: li.ownerDocument === document };",
    );
    assert_eq!(
        s(&mut r, "res"),
        r#"{"n":3,"last":"three","cls":"x y","data":"7","ds":"7","q":1,"closest":"app","matches":true,"same":true,"html":true,"tag":"LI","name":"LI","parent":true,"doc":true}"#
    );
    run(&mut r, "ul.insertBefore(li, ul.firstElementChild); li.remove(); ul.innerHTML = '<b>x</b><i>y</i>'; globalThis.after = [ul.children.length, ul.textContent, li.isConnected];");
    assert_eq!(s(&mut r, "after"), r#"[2,"xy",false]"#);
}

#[test]
fn dom_errors_are_domexceptions() {
    let mut r = rt("<div id=a><span id=b></span></div>");
    run(
        &mut r,
        "const a = document.getElementById('a'), b = document.getElementById('b'); globalThis.errs = [];
         try { b.appendChild(a); } catch (e) { errs.push([e instanceof DOMException, e.name]); }
         try { a.removeChild(document.body); } catch (e) { errs.push(e.name); }
         try { document.querySelector('div >'); } catch (e) { errs.push(e.name); }",
    );
    assert_eq!(s(&mut r, "errs"), r#"[[true,"HierarchyRequestError"],"NotFoundError","SyntaxError"]"#);
}

#[test]
fn inline_style_proxy_reads_and_writes_the_style_attribute() {
    let mut r = rt("<div id=d style='color: red'></div>");
    run(&mut r, "const d = document.getElementById('d'); d.style.backgroundColor = 'blue'; d.style.setProperty('--x', '1px'); d.style.marginTop = '4px'; globalThis.out = [d.getAttribute('style'), d.style.color, d.style.getPropertyValue('--x'), d.style.length]; d.style.color = ''; globalThis.out2 = d.style.cssText;");
    assert_eq!(s(&mut r, "out"), r#"["color: red; background-color: blue; --x: 1px; margin-top: 4px;","red","1px",4]"#);
    assert_eq!(s(&mut r, "out2"), "background-color: blue; --x: 1px; margin-top: 4px;");
}

#[test]
fn computed_style_and_geometry_reflect_layout() {
    let mut r = rt("<style>body{margin:0} #box{width:120px;height:40px;margin:10px;padding:5px;color:rgb(1,2,3);display:flex}</style><div id=box></div>");
    run(
        &mut r,
        "const b = document.getElementById('box'); const cs = getComputedStyle(b); const rc = b.getBoundingClientRect();
         globalThis.g = { w: cs.width, c: cs.color, d: cs.display, rect: [rc.x, rc.y, rc.width, rc.height], ow: b.offsetWidth, oh: b.offsetHeight, mt: cs.marginTop, via: cs.getPropertyValue('padding-left') };",
    );
    assert_eq!(s(&mut r, "g"), r#"{"w":"120px","c":"rgb(1, 2, 3)","d":"flex","rect":[10,10,130,50],"ow":130,"oh":50,"mt":"10px","via":"5px"}"#);
    run(&mut r, "document.getElementById('box').style.width = '200px'; globalThis.w2 = document.getElementById('box').offsetWidth;");
    assert_eq!(s(&mut r, "w2"), "210", "layout is recomputed synchronously after a DOM change");
}

#[test]
fn events_capture_target_bubble_order_and_control() {
    let mut r = rt("<div id=outer><div id=inner><button id=btn>x</button></div></div>");
    run(
        &mut r,
        "globalThis.log = []; const $ = id => document.getElementById(id);
         const add = (el, name, capture, extra) => el.addEventListener('click', e => { log.push(name + ':' + e.eventPhase); extra && extra(e); }, capture);
         add(window, 'win-cap', true); add(document, 'doc-cap', true); add($('outer'), 'outer-cap', true); add($('inner'), 'inner-cap', true);
         add($('btn'), 'btn-cap', true); add($('btn'), 'btn-bub', false);
         add($('inner'), 'inner-bub', false); add($('outer'), 'outer-bub', false); add(document, 'doc-bub', false); add(window, 'win-bub', false);
         $('btn').click();",
    );
    assert_eq!(
        s(&mut r, "log.join(' ')"),
        "win-cap:1 doc-cap:1 outer-cap:1 inner-cap:1 btn-cap:2 btn-bub:2 inner-bub:3 outer-bub:3 doc-bub:3 win-bub:3"
    );
    run(
        &mut r,
        "log.length = 0; const stop = e => e.stopPropagation(); $('inner').addEventListener('click', stop); $('btn').click(); globalThis.a = log.join(' ');
         $('inner').removeEventListener('click', stop); log.length = 0; let n = 0; $('btn').addEventListener('click', () => n++, { once: true }); $('btn').click(); $('btn').click(); globalThis.b = n;
         const ev = new CustomEvent('ping', { bubbles: true, cancelable: true, detail: { k: 1 } }); globalThis.d = []; document.addEventListener('ping', e => { d.push(e.detail.k, e.target.id); e.preventDefault(); });
         globalThis.ret = $('btn').dispatchEvent(ev); globalThis.prevented = ev.defaultPrevented;",
    );
    assert_eq!(s(&mut r, "a"), "win-cap:1 doc-cap:1 outer-cap:1 inner-cap:1 btn-cap:2 btn-bub:2 inner-bub:3", "stopPropagation at inner halts the bubble");
    assert_eq!(s(&mut r, "b"), "1", "once listener fires a single time");
    assert_eq!(s(&mut r, "[d, ret, prevented]"), r#"[[1,"btn"],false,true]"#);
}

#[test]
fn handler_properties_stop_immediate_and_listener_errors() {
    let mut r = rt("<button id=b>x</button>");
    run(
        &mut r,
        "const b = document.getElementById('b'); globalThis.log = [];
         b.onclick = () => log.push('prop'); b.addEventListener('click', e => { log.push('l1'); e.stopImmediatePropagation(); }); b.addEventListener('click', () => log.push('l2'));
         b.click(); b.onclick = null; log.push('|');
         b.addEventListener('mousedown', () => { throw new Error('boom'); }); b.addEventListener('mousedown', () => log.push('after-throw'));
         globalThis.winErr = []; addEventListener('error', e => winErr.push(e.message)); b.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));",
    );
    assert_eq!(s(&mut r, "log.join(',')"), "prop,l1,|,after-throw", "onclick ran first (registered first), stopImmediate skipped l2, a throwing listener does not stop others");
    assert_eq!(s(&mut r, "winErr"), r#"["boom"]"#, "uncaught listener errors surface as window 'error' events");
    assert!(logs(&mut r).iter().any(|l| l.starts_with("error:Uncaught") && l.contains("boom")));
}

#[test]
fn platform_dispatch_reaches_listeners_with_event_data() {
    let mut r = rt("<style>body{margin:0}#t{width:50px;height:50px}</style><div id=t></div>");
    run(&mut r, "globalThis.got = []; const t = document.getElementById('t'); t.addEventListener('pointerdown', e => got.push([e.type, e.clientX, e.clientY, e.button, e.isTrusted, e instanceof MouseEvent]));\
        document.addEventListener('keydown', e => { got.push([e.key, e.code, e.ctrlKey, e.target === document.body || e.target === window]); if (e.key === 'x') e.preventDefault(); });");
    let t = r.engine.borrow().doc.get_element_by_id("t").unwrap();
    r.dispatch("pointerdown", Some(t), &serde_json::json!({"clientX": 12.5, "clientY": 7, "button": 0}));
    let body = r.engine.borrow().doc.body().unwrap();
    let ok = r.dispatch("keydown", Some(body), &serde_json::json!({"key": "a", "code": "KeyA", "ctrlKey": true}));
    let prevented = !r.dispatch("keydown", Some(body), &serde_json::json!({"key": "x", "code": "KeyX"}));
    assert!(ok && prevented, "dispatch reports whether default was prevented");
    assert_eq!(s(&mut r, "got"), r#"[["pointerdown",12.5,7,0,true,true],["a","KeyA",true,true],["x","KeyX",false,true]]"#);
}

#[test]
fn checkbox_radio_label_and_form_activation() {
    let mut r = rt("<form id=f><input id=c type=checkbox><label for=c id=l>lbl</label><input type=radio name=g id=r1 checked><input type=radio name=g id=r2><button id=s>go</button><input id=t value=abc></form>");
    run(
        &mut r,
        "const $ = id => document.getElementById(id); globalThis.log = [];
         for (const id of ['c', 'r1', 'r2']) { $(id).addEventListener('input', () => log.push('input:' + id)); $(id).addEventListener('change', () => log.push('change:' + id + ':' + $(id).checked)); }
         $('f').addEventListener('submit', e => { e.preventDefault(); log.push('submit'); });
         $('l').click(); $('r2').click(); globalThis.state = [$('c').checked, $('r1').checked, $('r2').checked]; $('s').click();
         $('c').addEventListener('click', e => e.preventDefault()); $('c').click(); globalThis.cancelled = $('c').checked;",
    );
    assert_eq!(s(&mut r, "log.join(',')"), "input:c,change:c:true,input:r2,change:r2:true,submit");
    assert_eq!(s(&mut r, "state"), "[true,false,true]", "radio group is exclusive");
    assert_eq!(s(&mut r, "cancelled"), "true", "preventDefault restores the toggle");
}

#[test]
fn form_control_values_selection_and_focus_events() {
    let mut r = rt("<input id=i value='héllo'><textarea id=t>line</textarea><select id=s><option value=a>A<option value=b selected>B</select><button id=b>x</button>");
    run(
        &mut r,
        "const $ = id => document.getElementById(id); globalThis.log = [];
         for (const id of ['i', 'b']) for (const ev of ['focus', 'blur', 'focusin', 'focusout']) $(id).addEventListener(ev, e => log.push(id + ':' + ev + ':' + (e.relatedTarget ? e.relatedTarget.id : '-')));
         $('i').focus(); $('b').focus(); globalThis.active = document.activeElement.id; $('b').blur(); globalThis.afterBlur = document.activeElement === document.body;
         $('i').value = 'new text'; $('i').setSelectionRange(1, 3); globalThis.sel = [$('i').selectionStart, $('i').selectionEnd, $('i').value];
         globalThis.vals = [$('t').value, $('s').value, $('s').selectedIndex, $('i').defaultValue]; $('s').value = 'a'; globalThis.sv = [$('s').value, $('s').selectedIndex];",
    );
    assert_eq!(
        s(&mut r, "log.join(' ')"),
        "i:focus:- i:focusin:- i:blur:b i:focusout:b b:focus:i b:focusin:i b:blur:- b:focusout:-"
    );
    assert_eq!(s(&mut r, "[active, afterBlur, sel, vals, sv]"), r#"["b",true,[1,3,"new text"],["line","b",1,"héllo"],["a",0]]"#);
}

#[test]
fn modules_resolve_relative_imports_and_support_top_level_await() {
    let mut r = rt("");
    r.add_module("main.js", "import { add, PI } from './lib/math.js'; import def, * as ns from './lib/more'; import { version } from 'rendrix'; const v = await Promise.resolve(5); globalThis.out = [add(v, 1), PI, def(), Object.keys(ns).join(), typeof version];".into());
    r.add_module("lib/math.js", "export const PI = 3; export function add(a, b) { return a + b; }".into());
    r.add_module("lib/more.js", "import { PI } from './math.js'; export default () => PI * 2; export const extra = 1;".into());
    r.run_module("main.js").expect("module graph evaluates");
    assert_eq!(s(&mut r, "out"), r#"[6,3,6,"default,extra","string"]"#);
    r.add_module("bad.js", "import './nope.js';".into());
    let e = r.run_module("bad.js").unwrap_err();
    assert!(e.message.to_lowercase().contains("nope") || e.message.to_lowercase().contains("resolv") || e.message.to_lowercase().contains("could not"), "{}", e.message);
    r.add_module("throws.js", "throw new TypeError('startup failure');".into());
    assert!(r.run_module("throws.js").unwrap_err().message.contains("startup failure"));
}

#[test]
fn jsx_runtime_builds_dom_with_props_events_and_children() {
    let mut r = rt("<div id=root></div>");
    r.add_module(
        "app.js",
        r#"import { jsx, jsxs, Fragment, render } from 'rendrix/jsx-runtime';
           let count = 0;
           const Button = props => jsx('button', { class: 'btn', onClick: props.onClick, children: props.children });
           const App = () => jsxs('div', { className: 'app', style: { color: 'red', '--k': '3px' }, 'data-x': 1, children: [
             jsx('h1', { children: 'Count: ' + count }), jsxs(Fragment, { children: [jsx('span', { children: 'a' }), null, false, jsx('span', { children: 2 })] }),
             jsx(Button, { onClick: () => { count++; }, children: 'inc' }), jsx('input', { value: 'v', disabled: true, ref: el => { globalThis.inputRef = el; } })] });
           render(App(), document.getElementById('root'));
           globalThis.btn = document.querySelector('button');"#
            .into(),
    );
    r.run_module("app.js").unwrap();
    assert_eq!(
        s(&mut r, "document.getElementById('root').innerHTML"),
        r#"<div class="app" style="color: red; --k: 3px;" data-x="1"><h1>Count: 0</h1><span>a</span><span>2</span><button class="btn">inc</button><input value="v" disabled></div>"#
    );
    run(&mut r, "btn.click();");
    assert_eq!(s(&mut r, "inputRef.value"), "v");
    assert_eq!(s(&mut r, "document.querySelector('.app').style.color"), "red");
}

#[test]
fn url_search_params_encoding_and_base64() {
    let mut r = rt("");
    run(
        &mut r,
        "const u = new URL('https://user:pw@Example.com:8080/a/b/../c?x=1&y=two%20words#frag'); const rel = new URL('../d?q=1', 'http://h.test/a/b/c'); const sp = new URLSearchParams({ a: '1 2', b: 'x&y' }); sp.append('a', '3');
         u.searchParams.set('z', 'é'); globalThis.res = { href: u.href, origin: u.origin, host: u.host, path: u.pathname, y: u.searchParams.get('y'), rel: rel.href, sp: sp.toString(), all: sp.getAll('a'), canParse: [URL.canParse('nope'), URL.canParse('/x', 'http://a')],
           enc: [...new TextEncoder().encode('héllo €😀')].join(' '), dec: new TextDecoder().decode(new TextEncoder().encode('héllo €😀')), b64: [btoa('hello'), atob('aGVsbG8=')] };",
    );
    assert_eq!(
        s(&mut r, "res"),
        r#"{"href":"https://user:pw@example.com:8080/a/c?x=1&y=two+words&z=%C3%A9#frag","origin":"https://example.com:8080","host":"example.com:8080","path":"/a/c","y":"two words","rel":"http://h.test/a/d?q=1","sp":"a=1+2&b=x%26y&a=3","all":["1 2","3"],"canParse":[false,true],"enc":"104 195 169 108 108 111 32 226 130 172 240 159 152 128","dec":"héllo €😀","b64":["aGVsbG8=","hello"]}"#
    );
}

#[test]
fn abort_controller_and_structured_clone() {
    let mut r = rt("");
    run(
        &mut r,
        "const c = new AbortController(); globalThis.log = []; c.signal.addEventListener('abort', () => log.push('abort')); c.abort(); c.abort(); globalThis.aborted = [c.signal.aborted, c.signal.reason.name];
         const src = { a: [1, { b: new Date(5) }], m: new Map([[1, new Set([2])]]), n: null }; src.self = src; const copy = structuredClone(src);
         globalThis.cl = [copy !== src, copy.self === copy, copy.a[1].b.getTime(), copy.m.get(1).has(2), (() => { try { structuredClone({ f() {} }); } catch (e) { return e.name; } })()];",
    );
    assert_eq!(s(&mut r, "[log, aborted, cl]"), r#"[["abort"],[true,"AbortError"],[true,true,5,true,"DataCloneError"]]"#);
}

#[test]
fn fetch_reads_bundled_assets_and_data_urls_and_aborts() {
    let mut assets = MapAssets::default();
    assets.0.insert("data/info.json".into(), br#"{"name":"rendrix","n":[1,2]}"#.to_vec());
    let mut r = rt_with("", assets);
    run(
        &mut r,
        "globalThis.res = {};
         fetch('data/info.json').then(r => { res.status = r.status; res.ct = r.headers.get('content-type'); return r.json(); }).then(j => { res.json = j; });
         fetch('data:text/plain;base64,aGk=').then(r => r.text()).then(t => { res.data = t; });
         fetch('missing.txt').then(r => { res.missing = [r.ok, r.status]; });
         const c = new AbortController(); fetch('data/info.json', { signal: c.signal }).catch(e => { res.abort = e.name; }); c.abort();
         fetch('./data/info.json', { method: 'POST', body: 'x' }).then(r => r.text()).then(t => { res.post = t.length; });",
    );
    pump(&mut r, 50);
    assert_eq!(
        s(&mut r, "Object.keys(res).sort().map(k => [k, res[k]])"),
        r#"[["abort","AbortError"],["ct","application/json"],["data","hi"],["json",{"name":"rendrix","n":[1,2]}],["missing",[false,404]],["post",28],["status",200]]"#
    );
}

#[test]
fn local_and_session_storage() {
    let mut r = rt("");
    run(&mut r, "localStorage.clear(); localStorage.setItem('a', 1); localStorage.b = 'two'; sessionStorage.s = 'x'; globalThis.out = [localStorage.getItem('a'), localStorage.b, localStorage.length, Object.keys(localStorage), 'a' in localStorage, localStorage.missing, sessionStorage.s]; delete localStorage.a; globalThis.out2 = [localStorage.length, localStorage.key(0)];");
    assert_eq!(s(&mut r, "out"), r#"["1","two",2,["a","b"],true,null,"x"]"#);
    assert_eq!(s(&mut r, "out2"), r#"[1,"b"]"#);
}

#[test]
fn mutation_observer_delivers_batched_records() {
    let mut r = rt("<div id=root><p id=p>x</p></div>");
    run(
        &mut r,
        "globalThis.recs = []; const root = document.getElementById('root');
         new MutationObserver(rs => { for (const m of rs) recs.push([m.type, m.target.id || m.target.nodeName, m.addedNodes.length, m.removedNodes.length, m.attributeName, m.oldValue]); })
           .observe(root, { childList: true, attributes: true, subtree: true, attributeOldValue: true, characterData: true });
         const p = document.getElementById('p'); p.setAttribute('title', 't1'); p.setAttribute('title', 't2'); root.appendChild(document.createElement('span')); p.firstChild.data = 'y'; root.removeChild(p);",
    );
    assert_eq!(
        s(&mut r, "recs"),
        r##"[["attributes","p",0,0,"title",null],["attributes","p",0,0,"title","t1"],["childList","root",1,0,null,null],["characterData","#text",0,0,null,null],["childList","root",0,1,null,null]]"##
    );
}

#[test]
fn resize_observer_and_media_queries_follow_the_viewport() {
    let mut r = rt("<style>body{margin:0} #a{width:50px;height:20px}</style><div id=a></div>");
    run(
        &mut r,
        "globalThis.sizes = []; globalThis.mq = []; const a = document.getElementById('a');
         new ResizeObserver(es => es.forEach(e => sizes.push([e.contentRect.width, e.contentRect.height]))).observe(a);
         const m = matchMedia('(max-width: 300px)'); mq.push(m.matches); m.addEventListener('change', e => mq.push(e.matches)); addEventListener('resize', () => mq.push('resized'));",
    );
    r.run_animation_frame(16.0);
    a_resize(&mut r);
    r.engine.borrow_mut().set_viewport(250.0, 300.0, 1.0);
    r.viewport_changed();
    assert_eq!(s(&mut r, "sizes"), "[[50,20],[80,20]]");
    assert_eq!(s(&mut r, "mq"), r#"[false,"resized",true]"#);
    assert_eq!(s(&mut r, "[innerWidth, innerHeight, devicePixelRatio]"), "[250,300,1]");
}

fn a_resize(r: &mut JsRuntime) {
    run(r, "document.getElementById('a').style.width = '80px';");
    r.run_animation_frame(32.0);
}

#[test]
fn unhandled_rejections_and_script_errors_are_reported() {
    let mut r = rt("");
    run(&mut r, "globalThis.seen = []; addEventListener('unhandledrejection', e => { seen.push(String(e.reason)); if (String(e.reason).includes('nobody')) e.preventDefault(); }); Promise.reject(new Error('nobody catches this'));");
    assert_eq!(s(&mut r, "seen.length"), "1");
    run(&mut r, "Promise.reject('plain');");
    assert!(logs(&mut r).iter().any(|l| l.contains("Uncaught (in promise)") && l.contains("plain")));
    let e = r.run_script("bad", "throw new RangeError('x is out of range');").unwrap_err();
    assert!(e.message.contains("x is out of range"), "{e}");
    assert!(r.run_script("syntax", "let = ;").is_err());
}

#[test]
fn console_formatting() {
    let mut r = rt("");
    run(&mut r, "console.log('a', 1, { b: [1, 2], c: 'x' }, null, undefined); console.warn('%s is %d years', 'Ann', 30, 'extra'); console.group('G'); console.info('in'); console.groupEnd(); console.error(new Error('bad'));");
    let l = logs(&mut r);
    assert_eq!(l[0], "log:a 1 { b: [ 1, 2 ], c: \"x\" } null undefined");
    assert_eq!(l[1], "warn:Ann is 30 years extra");
    assert_eq!(l[2], "log:G");
    assert_eq!(l[3], "info:  in");
    assert!(l[4].starts_with("error:Error: bad"), "{}", l[4]);
}

#[test]
fn wrappers_are_collected_and_native_nodes_freed() {
    let mut r = rt("<div id=host></div>");
    run(&mut r, "const host = document.getElementById('host'); for (let i = 0; i < 600; i++) { const d = document.createElement('div'); d.textContent = 'n' + i; host.appendChild(d); host.removeChild(d); }");
    r.collect_garbage();
    run(&mut r, "1;"); // finalizer callbacks run as jobs after a collection
    r.collect_garbage();
    run(&mut r, "1;");
    r.engine.borrow_mut().doc.collect_garbage();
    let live = r.engine.borrow().doc.node_count();
    assert!(live < 40, "600 created+removed elements (and their text nodes) must not accumulate: {live} nodes alive");
    // A node that is still referenced from script survives sweeps, with its listeners and identity.
    run(&mut r, "globalThis.kept = document.createElement('p'); kept.addEventListener('x', () => { globalThis.fired = true; });");
    r.collect_garbage();
    run(&mut r, "1;");
    r.engine.borrow_mut().doc.collect_garbage();
    run(&mut r, "kept.dispatchEvent(new Event('x'));");
    assert_eq!(s(&mut r, "fired"), "true");
    assert_eq!(s(&mut r, "document.getElementById('host') === document.getElementById('host')"), "true");
}

#[test]
fn unsupported_apis_are_honestly_absent_or_throw() {
    let mut r = rt("<canvas id=c width=10 height=10></canvas>");
    assert_eq!(s(&mut r, "[typeof WebSocket, typeof XMLHttpRequest, typeof IntersectionObserver, typeof WebGLRenderingContext]"), r#"["undefined","undefined","undefined","undefined"]"#);
    assert_eq!(s(&mut r, "document.getElementById('c').getContext('2d')"), "null");
    run(&mut r, "globalThis.e = (() => { try { document.body.attachShadow({}); } catch (e) { return e.name; } })();");
    assert_eq!(s(&mut r, "e"), "NotSupportedError");
}

#[test]
fn filesystem_access_is_permission_gated() {
    let dir = std::env::temp_dir().join(format!("rendrix-fs-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "hello").unwrap();
    let mut r = rt("");
    r.host.borrow_mut().permissions = Permissions { network: false, fs_read: vec![dir.clone()], fs_write: vec![] };
    let p = dir.join("a.txt").to_string_lossy().replace('\\', "/");
    run(
        &mut r,
        &format!(
            "globalThis.res = []; rendrix.fs.readTextFile('{p}').then(t => res.push(t)); rendrix.fs.writeTextFile('{p}', 'x').catch(e => res.push('write denied'));
             rendrix.fs.readTextFile('C:/Windows/win.ini').catch(e => res.push('read denied')); fetch('https://example.com').catch(e => res.push('net denied'));"
        ),
    );
    pump(&mut r, 30);
    let out = s(&mut r, "res");
    assert!(out.contains("hello") && out.contains("write denied") && out.contains("read denied") && out.contains("net denied"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn crypto_random_values_come_from_the_os_and_look_random() {
    let mut r = rt("");
    run(&mut r, "const a = crypto.randomUUID(), b = crypto.randomUUID(); const buf = crypto.getRandomValues(new Uint8Array(4096)); const distinct = new Set(buf).size;
                 globalThis.out = [/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(a), a !== b, distinct > 200];");
    assert_eq!(s(&mut r, "out"), "[true,true,true]");
}
