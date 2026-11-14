//! `rendrix-runtime bench`: measures the frame pipeline on a synthetic list UI.
//!
//! Every number is wall-clock on this machine and this build; run it with `--release`. The scenarios
//! separate the CPU stages (style, layout, paint list) from GPU submission so regressions are attributable.

use crate::bundle::AppBundle;
use crate::headless::Headless;
use std::time::{Duration, Instant};

fn bundle(items: usize) -> AppBundle {
    let html = r#"<html><head><style>
      body{margin:0;background:#101010;color:#ddd;font:14px "Segoe UI",sans-serif}
      #list{padding:8px;display:flex;flex-direction:column;gap:4px}
      .item{display:flex;align-items:center;gap:12px;padding:6px 10px;background:#1a1a1a;border:1px solid #262626;border-radius:6px;transition:background-color 150ms}
      .item.hot{background:#24402a}
      .idx{width:36px;color:#888;font-size:12px}
      .label{flex:1}
      .tag{padding:2px 8px;border-radius:10px;background:#2b2b2b;font-size:11px}
      .item button{padding:4px 10px;border:0;border-radius:4px;background:#e5e5e5}
    </style></head><body><div id=list></div><script type=module src=main.js></script></body></html>"#;
    let js = format!(
        "const list = document.getElementById('list'); const frag = document.createDocumentFragment();
         for (let i = 0; i < {items}; i++) {{ const d = document.createElement('div'); d.className = 'item'; d.id = 'i' + i;
           d.innerHTML = '<span class=idx>' + i + '</span><span class=label>Item number ' + i + ' with some descriptive text</span><span class=tag>tag ' + (i % 7) + '</span><button>Open</button>'; frag.appendChild(d); }}
         list.appendChild(frag);"
    );
    let mut b = AppBundle::from_json(r#"{"format":"rendrix-app/1","name":"bench","html":"","width":1000,"height":700}"#).unwrap();
    b.html = html.to_string();
    b.modules.insert("main.js".into(), js);
    b
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

struct Sample {
    name: &'static str,
    total_ms: f64,
    style_ms: f64,
    layout_ms: f64,
    paint_ms: f64,
    gpu_ms: f64,
}

pub fn run(items: usize, iterations: usize) -> Result<(), String> {
    let t0 = Instant::now();
    let mut h = Headless::new(&bundle(items), (1000, 700), 1.0)?;
    let startup = t0.elapsed();
    let first = Instant::now();
    h.session.frame();
    let first_frame = first.elapsed();
    h.settle(Duration::from_secs(1));
    let nodes = h.session.engine.borrow().doc.node_count();
    println!("Rendrix pipeline benchmark: {items} list items, {nodes} DOM nodes, 1000x700 @1x on {}", h.adapter);
    println!("startup (runtime + JS builds the DOM + first frame): {:.1} ms ({:.1} ms of it the first frame)\n", startup.as_secs_f64() * 1000.0, first_frame.as_secs_f64() * 1000.0);

    let mut results: Vec<Sample> = Vec::new();
    let mut measure = |h: &mut Headless, name: &'static str, mut mutate: Box<dyn FnMut(&mut Headless, usize)>| {
        let (mut total, mut style, mut layout, mut paint, mut gpu) = (vec![], vec![], vec![], vec![], vec![]);
        // Warm up caches (glyph atlas, paragraph cache) before measuring.
        for i in 0..3 {
            mutate(h, i);
            h.session.frame();
            h.render_timed();
        }
        for i in 0..iterations {
            mutate(h, i + 3);
            let t = Instant::now();
            h.session.frame();
            let cpu = t.elapsed().as_secs_f64() * 1000.0;
            let st = h.session.engine.borrow().stats.clone();
            let g = h.render_timed().as_secs_f64() * 1000.0;
            total.push(cpu + g);
            style.push(st.last_style_micros as f64 / 1000.0);
            layout.push(st.last_layout_micros as f64 / 1000.0);
            paint.push(st.last_paint_micros as f64 / 1000.0);
            gpu.push(g);
        }
        results.push(Sample { name, total_ms: median(&mut total), style_ms: median(&mut style), layout_ms: median(&mut layout), paint_ms: median(&mut paint), gpu_ms: median(&mut gpu) });
    };

    measure(&mut h, "idle frame (nothing changed)", Box::new(|_, _| {}));
    measure(
        &mut h,
        "toggle a class on 1 item",
        Box::new(move |h, i| {
            let id = h.session.engine.borrow().doc.get_element_by_id(&format!("i{}", (i * 37) % items)).unwrap();
            h.session.engine.borrow_mut().doc.toggle_class(id, "hot", None);
        }),
    );
    measure(
        &mut h,
        "change text of 1 item",
        Box::new(move |h, i| {
            let mut e = h.session.engine.borrow_mut();
            let id = e.doc.get_element_by_id(&format!("i{}", (i * 53) % items)).unwrap();
            let label = e.doc.query_selector(id, ".label").unwrap().unwrap();
            e.doc.set_text_content(label, &format!("Updated {i}"));
        }),
    );
    measure(
        &mut h,
        "hover change (pointer moves between items)",
        Box::new(|h, i| {
            h.session.pointer_move(500.0, 30.0 + (i % 12) as f32 * 34.0);
        }),
    );
    measure(
        &mut h,
        "scroll by one wheel notch",
        Box::new(|h, i| {
            h.session.wheel(500.0, 300.0, 0.0, if i % 2 == 0 { 48.0 } else { -48.0 });
        }),
    );
    measure(
        &mut h,
        "add + remove 10 items",
        Box::new(|h, i| {
            let mut e = h.session.engine.borrow_mut();
            let list = e.doc.get_element_by_id("list").unwrap();
            for k in 0..10 {
                let d = e.doc.create_element("div");
                e.doc.set_attr(d, "class", "item");
                e.doc.set_inner_html(d, &format!("<span class=idx>{k}</span><span class=label>added {i}</span>"));
                e.doc.append_child(list, d).unwrap();
            }
            let kids: Vec<_> = e.doc.children(list).collect();
            for k in kids.iter().rev().take(10) {
                let _ = e.doc.remove_child(list, *k);
            }
        }),
    );
    measure(
        &mut h,
        "viewport resize (full restyle + relayout)",
        Box::new(|h, i| {
            h.session.resize(1000.0 - (i % 2) as f32 * 40.0, 700.0, 1.0);
        }),
    );
    measure(
        &mut h,
        "JS handler: classList.toggle via script",
        Box::new(move |h, i| {
            let _ = h.session.js.eval(&format!("document.getElementById('i{}').classList.toggle('hot')", (i * 11) % items));
        }),
    );

    println!("{:<44} {:>9} {:>8} {:>8} {:>8} {:>8}", "scenario (median of N)", "total ms", "style", "layout", "paint", "GPU");
    for r in &results {
        println!("{:<44} {:>9.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2}", r.name, r.total_ms, r.style_ms, r.layout_ms, r.paint_ms, r.gpu_ms);
    }
    let e = h.session.engine.borrow();
    let list = e.display_list();
    println!("\nlast display list: {} items; GPU batches {}, instances {}", list.items.len(), h.render_stats().1, h.render_stats().0);
    println!("budget for 60 FPS = 16.7 ms, 120 FPS = 8.3 ms (CPU + GPU submission, excluding vsync wait)");
    Ok(())
}
