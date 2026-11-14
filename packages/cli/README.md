# @bytethedeveloper/rendrix (CLI)

The `rendrix` command builds and runs TypeScript applications on the Rendrix engine.

```sh
rendrix init my-app     # new project from the `app` template
rendrix dev             # build, open a native window, rebuild + restart on change
rendrix build           # type-check + compile to dist/<name>.rdxapp
rendrix run             # build and run once   (--headless --screenshot out.png --script steps.json --size WxH)
rendrix check           # type-check only
rendrix package         # standalone executable (needs a built runtime; see below)
rendrix doctor          # toolchain, GPU adapter and project checks
```

The CLI drives the Rust runtime (`rendrix-runtime`). It looks for it in `RENDRIX_RUNTIME`, then
`engine/target/{release,debug}`, then `$RENDRIX_HOME/runtime/<platform>-<arch>/`. Build it with
`cargo build --release -p rendrix-runtime --manifest-path engine/Cargo.toml`.

`rendrix build` compiles every module to ES2022 JavaScript (the runtime never sees TypeScript), resolves
imports (ESM packages from `node_modules` only), and writes a `rendrix-app/1` JSON bundle. Type errors fail
the build unless `--no-typecheck` is given. Source: `src/app/`. Tests: `npm test` in this directory.

`src/signing.mjs` and `src/installer/` are code-signing and NSIS helpers from earlier tooling; they are not
wired into any command yet.
