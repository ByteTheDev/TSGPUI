# Safety and trust model

- This project never edits existing application installations or executables; packaging **copies** a host to an output directory before appending the bundle.
- No cleanup tools, disk wipes, uninstallers, system registry edits, elevation requests, or background services are included.
- The development command watches only project files and restarts only the host process it spawned.
- The native binary must be trusted: use a local source build or a checksum-verified CI artifact.
- The current QuickJS state/actions API exposes no filesystem/network APIs. Each backend load, action, or state write has a 2-second wall-clock limit and a 32 MiB heap limit; an action that exceeds the limit is aborted and its state is rolled back. Limits do not cover memory used outside the JS heap, so still do not execute untrusted backend code.
- CLI `init` refuses to overwrite a nonempty project folder. The CLI resolves symlinks for the config, HTML, CSS, backend, image assets, and output directory, and rejects any that point outside the project. It also refuses to write through a symlink at the output path. The check runs once at build time, so a project edited concurrently could still race it; do not build untrusted projects.
- `tsgpui preview` runs the backend in a browser page, not in QuickJS, so its limits are the browser's, not the host's. The page's content security policy allows script from the page itself (the preview needs `unsafe-eval` to run the backend), and network requests only to the exact hosts in `permissions.network`. The preview server listens on 127.0.0.1 only, and it serves only the page and a version number. Do not treat a preview as a sandbox for untrusted backend code.
- Assembling a Windows executable by appending an overlay invalidates any existing signature. Use an unsigned prebuilt host or a signing process after packaging, and test antivirus compatibility.
- The release packaging replaces the **generated output file** when rebuilt; keep backups of outputs you need to retain. It never targets existing installed apps.
- Dependencies, gpui versions and GitHub Actions permissions should be audited/pinned before public releases.
