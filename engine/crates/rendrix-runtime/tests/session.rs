//! Platform-independent integration tests: a real bundle (document + ES modules + assets) driven
//! through `Session` with the same input calls the window driver makes.

use rendrix_runtime::bundle::AppBundle;
use rendrix_runtime::headless::Headless;
use rendrix_runtime::session::{KeyInput, MemoryClipboard, Mods, Session};
use std::time::Duration;

fn bundle(html: &str, main_js: &str) -> AppBundle {
    let mut b = AppBundle::from_json(r#"{"format":"rendrix-app/1","name":"it","html":"","width":400,"height":300}"#).unwrap();
    b.html = html.to_string();
    b.modules.insert("main.js".into(), main_js.to_string());
    b
}

fn session(html: &str, js: &str) -> Session {
    let mut s = Session::new(&bundle(html, js), Box::new(MemoryClipboard::default()), (400.0, 300.0, 1.0)).expect("session");
    s.js.host.borrow_mut().echo_console = false;
    s.frame();
    s
}

fn eval(s: &mut Session, expr: &str) -> String {
    match s.js.eval(&format!("(()=>{{ const v = ({expr}); return typeof v === 'string' ? v : JSON.stringify(v); }})()")) {
        Ok(rendrix_js::Val::Str(v)) => v,
        other => panic!("eval({expr}) -> {other:?}"),
    }
}

fn key(k: &str) -> KeyInput {
    KeyInput { key: k.into(), code: k.into(), text: None, repeat: false, mods: Mods::default() }
}

fn typed(s: &mut Session, text: &str) {
    for ch in text.chars() {
        let k = KeyInput { key: ch.to_string(), code: String::new(), text: Some(ch.to_string()), repeat: false, mods: Mods::default() };
        s.key_down(k.clone());
        s.key_up(k);
    }
}

fn center(s: &mut Session, sel: &str) -> (f32, f32) {
    s.frame();
    let id = s.query(sel).unwrap_or_else(|| panic!("no {sel}"));
    s.center_of(id).unwrap_or_else(|| panic!("{sel} has no box"))
}

const PAGE: &str = r#"<html><head><style>
  body{margin:0;font:14px sans-serif} .row{display:flex;gap:8px;padding:8px}
  button{width:90px;height:30px} button:hover{background:#0a0} input{width:160px;height:24px}
  #list{height:100px;overflow:auto} #list div{height:30px} #far{margin-top:900px;height:20px}
</style></head><body>
  <div class="row"><button id="a">A</button><button id="b">B</button><input id="name" placeholder="type here"><input id="other"><label id="lbl" for="chk">check me</label><input id="chk" type="checkbox"></div>
  <textarea id="ta" style="width:200px;height:60px"></textarea>
  <div id="list"><div>1</div><div>2</div><div>3</div><div>4</div><div>5</div><div>6</div><div>7</div><div>8</div></div>
  <div id="far"></div>
  <script type="module" src="main.js"></script></body></html>"#;

const JS: &str = r#"
  globalThis.log = [];
  const $ = id => document.getElementById(id);
  for (const id of ['a','b']) {
    $(id).addEventListener('click', e => log.push('click:' + id + ':' + e.detail));
    $(id).addEventListener('mouseenter', () => log.push('enter:' + id));
    $(id).addEventListener('mouseleave', () => log.push('leave:' + id));
  }
  for (const id of ['name','other']) {
    $(id).addEventListener('input', e => log.push('input:' + id + ':' + $(id).value + ':' + e.inputType));
    $(id).addEventListener('change', () => log.push('change:' + id + ':' + $(id).value));
    $(id).addEventListener('focus', () => log.push('focus:' + id));
    $(id).addEventListener('blur', () => log.push('blur:' + id));
  }
  $('chk').addEventListener('change', () => log.push('chk:' + $('chk').checked));
  document.addEventListener('keydown', e => { if (e.key === 'F9') { e.preventDefault(); log.push('f9-handled'); } });
  $('a').addEventListener('keydown', e => log.push('keydown-on-a:' + e.key));
"#;

#[test]
fn hover_click_and_activation_flow_through_dom_events() {
    let mut s = session(PAGE, JS);
    let (ax, ay) = center(&mut s, "#a");
    let (bx, by) = center(&mut s, "#b");
    s.pointer_move(ax, ay);
    assert_eq!(eval(&mut s, "document.getElementById('a').matches(':hover')"), "true");
    s.pointer_move(bx, by);
    assert_eq!(eval(&mut s, "[document.getElementById('a').matches(':hover'), document.getElementById('b').matches(':hover')]"), "[false,true]");
    s.frame();
    assert_eq!(eval(&mut s, "getComputedStyle(document.getElementById('b')).backgroundColor"), "rgb(0, 170, 0)", "pseudo-class styles apply from real pointer state");
    s.pointer_down(bx, by, 0);
    s.pointer_up(bx, by, 0);
    s.pointer_down(bx, by, 0);
    s.pointer_up(bx, by, 0);
    assert_eq!(eval(&mut s, "log.join(' ')"), "enter:a leave:a enter:b click:b:1 click:b:2");
    // Press on one element, release on another: click goes to the common ancestor, not to either button.
    eval(&mut s, "log.length = 0");
    s.pointer_down(ax, ay, 0);
    s.pointer_up(bx, by, 0);
    assert_eq!(eval(&mut s, "log.filter(x => x.startsWith('click')).length"), "0");
    // Clicking the label toggles the checkbox it points to.
    let (lx, ly) = center(&mut s, "#lbl");
    s.pointer_down(lx, ly, 0);
    s.pointer_up(lx, ly, 0);
    assert_eq!(eval(&mut s, "[document.getElementById('chk').checked, log.at(-1)]"), r#"[true,"chk:true"]"#);
}

#[test]
fn keyboard_focus_tab_order_and_typing() {
    let mut s = session(PAGE, JS);
    let (nx, ny) = center(&mut s, "#name");
    s.pointer_down(nx, ny, 0);
    s.pointer_up(nx, ny, 0);
    assert_eq!(eval(&mut s, "document.activeElement.id"), "name");
    typed(&mut s, "héllo");
    s.key_down(key("Backspace"));
    assert_eq!(eval(&mut s, "document.getElementById('name').value"), "héll");
    // Navigation and selection keys.
    s.key_down(key("Home"));
    s.key_down(KeyInput { mods: Mods { shift: true, ..Default::default() }, ..key("ArrowRight") });
    typed(&mut s, "J");
    assert_eq!(eval(&mut s, "document.getElementById('name').value"), "Jéll");
    // Tab moves focus forward, fires change on the field being left, then focus on the next.
    s.key_down(key("Tab"));
    assert_eq!(eval(&mut s, "document.activeElement.id"), "other");
    assert!(eval(&mut s, "log.join(' ')").contains("change:name:Jéll blur:name focus:other"), "{}", eval(&mut s, "log.join(' ')"));
    s.key_down(KeyInput { mods: Mods { shift: true, ..Default::default() }, ..key("Tab") });
    assert_eq!(eval(&mut s, "document.activeElement.id"), "name");
    // input events carry inputType.
    assert!(eval(&mut s, "log.join(' ')").contains("input:name:h:insertText"));
    assert!(eval(&mut s, "log.join(' ')").contains("deleteContentBackward"));
    // preventDefault on keydown cancels default handling.
    s.key_down(key("F9"));
    assert!(eval(&mut s, "log.join(' ')").ends_with("f9-handled"));
}

#[test]
fn enter_and_space_activate_focused_buttons() {
    let mut s = session(PAGE, JS);
    s.key_down(key("Tab"));
    assert_eq!(eval(&mut s, "document.activeElement.id"), "a");
    s.key_down(key("Enter"));
    s.key_down(key(" "));
    s.key_up(key(" "));
    assert!(eval(&mut s, "log.join(' ')").contains("keydown-on-a:Enter click:a:0 keydown-on-a: "), "{}", eval(&mut s, "log.join(' ')"));
    assert_eq!(eval(&mut s, "log.filter(x => x.startsWith('click:a')).length"), "2", "Enter and Space each activate once");
}

#[test]
fn clipboard_and_selection_editing() {
    let mut s = session(PAGE, JS);
    let (nx, ny) = center(&mut s, "#name");
    s.pointer_down(nx, ny, 0);
    s.pointer_up(nx, ny, 0);
    typed(&mut s, "copy me");
    let ctrl = Mods { ctrl: true, ..Default::default() };
    s.key_down(KeyInput { mods: ctrl, ..key("a") });
    s.key_down(KeyInput { mods: ctrl, ..key("c") });
    assert_eq!(eval(&mut s, "document.getElementById('name').selectionStart + ',' + document.getElementById('name').selectionEnd"), "0,7");
    s.key_down(KeyInput { mods: ctrl, ..key("x") });
    assert_eq!(eval(&mut s, "document.getElementById('name').value"), "");
    s.key_down(KeyInput { mods: ctrl, ..key("v") });
    s.key_down(KeyInput { mods: ctrl, ..key("v") });
    assert_eq!(eval(&mut s, "document.getElementById('name').value"), "copy mecopy me");
    // Double click selects a word, typing replaces it.
    s.key_down(KeyInput { mods: ctrl, ..key("a") });
    typed(&mut s, "one two");
    let (nx, ny) = center(&mut s, "#name");
    let _ = (nx, ny);
    s.js.set_focus(None);
    assert!(eval(&mut s, "log.join(' ')").contains("blur:name"));
}

#[test]
fn textarea_multiline_editing_and_ime_commit() {
    let mut s = session(PAGE, JS);
    let (x, y) = center(&mut s, "#ta");
    s.pointer_down(x, y, 0);
    s.pointer_up(x, y, 0);
    typed(&mut s, "ab");
    s.key_down(key("Enter"));
    typed(&mut s, "cd");
    s.text_commit("日本");
    assert_eq!(eval(&mut s, "document.getElementById('ta').value"), "ab\ncd日本");
    assert_eq!(eval(&mut s, "document.getElementById('ta').value.length"), "7");
}

#[test]
fn wheel_scrolls_the_innermost_container_then_the_document() {
    let mut s = session(PAGE, JS);
    let (lx, ly) = center(&mut s, "#list");
    s.wheel(lx, ly, 0.0, 70.0);
    s.frame();
    assert_eq!(eval(&mut s, "document.getElementById('list').scrollTop"), "70");
    // Scroll the list to its end; further wheel input falls through to the document.
    s.wheel(lx, ly, 0.0, 1000.0);
    s.frame();
    assert_eq!(eval(&mut s, "document.getElementById('list').scrollTop"), "140", "clamped: 8*30 - 100");
    assert_eq!(eval(&mut s, "scrollY"), "0");
    s.wheel(lx, ly, 0.0, 120.0);
    s.frame();
    assert_eq!(eval(&mut s, "scrollY"), "120", "list is at its end so the page scrolls");
    // Items scrolled out of view are no longer hit-testable where they used to be.
    assert_eq!(eval(&mut s, "document.elementFromPoint(20, 10).tagName.length > 0"), "true");
}

#[test]
fn wheel_event_can_be_cancelled_by_the_page() {
    let mut s = session(PAGE, "document.addEventListener('wheel', e => { e.preventDefault(); globalThis.dy = e.deltaY; });");
    s.wheel(50.0, 250.0, 0.0, 200.0);
    s.frame();
    assert_eq!(eval(&mut s, "[scrollY, dy]"), "[0,200]");
}

#[test]
fn timers_fetch_and_animation_frames_drive_visible_updates() {
    let html = "<html><body><div id=out>idle</div><script type=module src=main.js></script></body></html>";
    let js = "const out = document.getElementById('out'); let n = 0; const step = () => { n++; out.textContent = 'frame ' + n; if (n < 3) requestAnimationFrame(step); }; requestAnimationFrame(step);
              setTimeout(() => { out.dataset.timer = 'fired'; }, 10);
              fetch('data/x.txt').then(r => r.text()).then(t => { out.dataset.fetched = t; });";
    let mut b = bundle(html, js);
    b.assets.insert("data/x.txt".into(), rendrix_runtime::bundle::Asset::Text { text: "payload".into() });
    let mut s = Session::new(&b, Box::new(MemoryClipboard::default()), (400.0, 300.0, 1.0)).unwrap();
    s.js.host.borrow_mut().echo_console = false;
    for _ in 0..40 {
        s.frame();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(eval(&mut s, "[document.getElementById('out').textContent, document.getElementById('out').dataset.timer, document.getElementById('out').dataset.fetched]"), r#"["frame 3","fired","payload"]"#);
}

#[test]
fn module_errors_surface_with_the_script_name() {
    let err = Session::new(&bundle("<html><body><script type=module src=main.js></script></body></html>", "throw new Error('boom at startup');"), Box::new(MemoryClipboard::default()), (400.0, 300.0, 1.0)).err().expect("startup error");
    assert!(err.contains("main.js") && err.contains("boom at startup"), "{err}");
    let err = Session::new(&bundle("<html><body><script type=module src=missing.js></script></body></html>", ""), Box::new(MemoryClipboard::default()), (400.0, 300.0, 1.0)).err().unwrap();
    assert!(err.contains("missing.js"), "{err}");
}

#[test]
fn gpu_pixels_reflect_interaction() {
    let html = "<html><head><style>body{margin:0;background:#000} #b{width:100px;height:60px;background:rgb(200,0,0)} #b.on{background:rgb(0,200,0)} #b:hover{outline:0}</style></head><body><div id=b></div><script type=module src=main.js></script></body></html>";
    let js = "document.getElementById('b').addEventListener('click', e => e.currentTarget.classList.add('on'));";
    let Ok(mut h) = Headless::new(&bundle(html, js), (200, 100), 2.0) else { eprintln!("no GPU, skipping"); return };
    h.settle(Duration::from_secs(1));
    let before = h.screenshot();
    assert_eq!((before.width, before.height), (400, 200), "2x scale renders at device resolution");
    assert_eq!(before.pixel(100, 60), [200, 0, 0, 255]);
    assert_eq!(before.pixel(300, 150), [0, 0, 0, 255]);
    h.click(50.0, 30.0);
    h.settle(Duration::from_secs(1));
    let after = h.screenshot();
    assert_eq!(after.pixel(100, 60), [0, 200, 0, 255], "click handler changed class → restyle → repaint");
    assert_eq!(after.pixel(300, 150), [0, 0, 0, 255]);
}
