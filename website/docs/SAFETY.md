# Safety and permissions

**Rendrix runs your application's code with the privileges of the user who starts it, plus a small set of
capability checks. It is not a sandbox for hostile code.** Only run applications you trust.

## What is enforced

| Capability | Default | How it is enforced |
|---|---|---|
| Network (`fetch` to `http(s)`) | **denied** | `permissions.network` in `rendrix.json`; otherwise `fetch` rejects with a `TypeError`. There is no per-host allow-list yet. |
| File reads | **denied** | Only through `rendrix.fs`; paths are canonicalised and must lie inside a directory listed in `permissions.fs_read`. Plain Web APIs cannot touch the filesystem. |
| File writes | **denied** | Same, with `permissions.fs_write`. |
| Relative `fetch`, images, fonts, stylesheets | bundled assets only | `DirAssets` and bundle assets reject `..` segments. |
| Process spawning, native add-ons, Node/Bun APIs | **do not exist** | There is no such API in the runtime. |
| Remote script/module loading | **do not exist** | The build refuses `http(s):`/`node:` imports; the module loader only reads the bundle. |

Covered by tests: `filesystem_access_is_permission_gated` (reads, writes, denied paths, denied network),
bundle trailer/format validation, and build-time rejection of Node built-ins and CommonJS.

## What is not enforced

* No memory or CPU limit on script execution: an infinite loop freezes the window (QuickJS memory cap is
  1 GiB, stack 2 MiB).
* QuickJS and native crates are not hardened against malicious inputs; treat untrusted images, fonts and
  HTML as a risk.
* Scripts can read the whole bundle and their own `localStorage` (a plain JSON file in the user data
  directory). Nothing is encrypted.
* A bundle appended to an executable is not signed or integrity-checked by the runtime. Code signing is not
  implemented.
* `crypto.getRandomValues` uses the OS CSPRNG, but `crypto.subtle` does not exist.

## Reporting

Open a private security advisory on the repository. Please include the bundle or script that reproduces
the behaviour.
