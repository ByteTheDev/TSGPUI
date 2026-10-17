# Safety and trust model

- This project never edits existing application installations or executables; packaging **copies** a host to an output directory before appending the bundle.
- No cleanup tools, disk wipes, uninstallers, system registry edits, elevation requests, or background services are included.
- The development command watches only project files and restarts only the host process it spawned.
- The native binary must be trusted: use a local source build or a checksum-verified CI artifact.
- The current QuickJS state/actions API exposes no filesystem/network APIs, but runtime resource limits are incomplete; do not execute untrusted backend code.
- CLI `init` refuses to overwrite a nonempty project folder. Project config input/output paths cannot intentionally traverse outside the project directory. Symlink containment is not yet enforced; do not build untrusted projects.
- Assembling a Windows executable by appending an overlay invalidates any existing signature. Use an unsigned prebuilt host or a signing process after packaging, and test antivirus compatibility.
- The release packaging replaces the **generated output file** when rebuilt; keep backups of outputs you need to retain. It never targets existing installed apps.
- Dependencies, gpui versions and GitHub Actions permissions should be audited/pinned before public releases.
