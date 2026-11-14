//! `rendrix-runtime`: run a Rendrix application.
//!
//! ```text
//! rendrix-runtime [run] <dir | app.rdxapp> [--headless] [--screenshot out.png] [--script steps.json] [--size WxH] [--scale S]
//! rendrix-runtime pack <dir | app.rdxapp> --out app.exe [--runtime path/to/rendrix-runtime.exe]
//! rendrix-runtime                         (runs the bundle attached to this executable)
//! ```

use rendrix_runtime::{bundle::AppBundle, headless::Headless, window};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn usage() -> ! {
    eprintln!(
        "usage:\n  rendrix-runtime [run] <dir | app.rdxapp> [--headless] [--screenshot out.png] [--script steps.json] [--size WxH] [--scale S]\n  rendrix-runtime pack <dir | app.rdxapp> --out app[.exe] [--runtime path]\n  rendrix-runtime            (run the bundle attached to this executable)"
    );
    std::process::exit(2);
}

struct Args {
    cmd: String,
    path: Option<PathBuf>,
    headless: bool,
    screenshot: Option<PathBuf>,
    script: Option<PathBuf>,
    size: Option<(u32, u32)>,
    scale: f32,
    out: Option<PathBuf>,
    runtime: Option<PathBuf>,
}

fn parse() -> Args {
    let mut it = std::env::args().skip(1).peekable();
    let mut a = Args { cmd: "run".into(), path: None, headless: false, screenshot: None, script: None, size: None, scale: 1.0, out: None, runtime: None };
    if let Some(first) = it.peek() {
        if first == "run" || first == "pack" || first == "doctor" || first == "bench" {
            a.cmd = it.next().unwrap();
        }
    }
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--headless" => a.headless = true,
            "--screenshot" => a.screenshot = it.next().map(PathBuf::from),
            "--script" => a.script = it.next().map(PathBuf::from),
            "--out" => a.out = it.next().map(PathBuf::from),
            "--runtime" => a.runtime = it.next().map(PathBuf::from),
            "--scale" => a.scale = it.next().and_then(|s| s.parse().ok()).unwrap_or_else(|| usage()),
            "--size" => {
                let s = it.next().unwrap_or_else(|| usage());
                let (w, h) = s.split_once('x').unwrap_or_else(|| usage());
                a.size = Some((w.parse().unwrap_or_else(|_| usage()), h.parse().unwrap_or_else(|_| usage())));
            }
            "-h" | "--help" => usage(),
            other if other.starts_with("--") => usage(),
            other => a.path = Some(PathBuf::from(other)),
        }
    }
    a
}

fn main() {
    let args = parse();
    if let Err(e) = real_main(args) {
        eprintln!("rendrix-runtime: {e}");
        std::process::exit(1);
    }
}

fn doctor() -> Result<(), String> {
    println!("Runtime: rendrix-runtime {}", env!("CARGO_PKG_VERSION"));
    println!("JavaScript engine: QuickJS (ES modules, async/await, promises)");
    match rendrix_gfx::GpuContext::headless() {
        Ok(ctx) => println!("GPU adapter: {}", ctx.adapter_name()),
        Err(e) => {
            println!("GPU adapter: no suitable adapter found ({e})");
            return Err("no GPU adapter".into());
        }
    }
    Ok(())
}

fn real_main(args: Args) -> Result<(), String> {
    if args.cmd == "doctor" {
        return doctor();
    }
    if args.cmd == "bench" {
        let items = args.path.as_ref().and_then(|p| p.to_str()).and_then(|s| s.parse().ok()).unwrap_or(1000);
        return rendrix_runtime::bench::run(items, 30);
    }
    let bundle = AppBundle::load(args.path.as_deref())?;
    if args.cmd == "pack" {
        let out = args.out.ok_or("pack needs --out <file>")?;
        let runtime = match args.runtime {
            Some(r) => r,
            None => std::env::current_exe().map_err(|e| e.to_string())?,
        };
        bundle.pack(&runtime, &out)?;
        println!("packed '{}' -> {}", bundle.name, out.display());
        return Ok(());
    }
    if args.headless || args.screenshot.is_some() || args.script.is_some() {
        let size = args.size.unwrap_or((bundle.width, bundle.height));
        let mut h = Headless::new(&bundle, size, args.scale)?;
        eprintln!("[rendrix] headless on {}", h.adapter);
        h.settle(Duration::from_secs(2));
        if let Some(script) = &args.script {
            let text = std::fs::read_to_string(script).map_err(|e| format!("{}: {e}", script.display()))?;
            let steps: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", script.display()))?;
            let dir = args.screenshot.as_deref().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
            let n = h.run_steps(&steps, &dir)?;
            eprintln!("[rendrix] ran {n} scripted steps");
        }
        if let Some(path) = &args.screenshot {
            h.settle(Duration::from_secs(1));
            h.screenshot().save_png(path)?;
            println!("[screenshot] {}", path.display());
        }
        return Ok(());
    }
    window::run(bundle)
}
