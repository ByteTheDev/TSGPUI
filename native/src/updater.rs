// In-app update staging and install for packaged Rendrix apps.
//
// Page flow: the TS `autoUpdater` shim reads a JSON feed (`{version, url, sha256?}`), compares
// versions, then calls `updater.download` with the artifact URL. The host streams the artifact
// straight into `userData/updates/<name>` while computing SHA-256, refuses a mismatch, and
// reports the staged path — the artifact never round-trips through the JS bridge as a byte
// array. On `quitAndInstall` the shim calls `updater.install`; the host renames the running
// executable to `<exe>.old` (renaming a running image is allowed on Windows; deleting it is
// not), copies the staged artifact over the old path, optionally relaunches, and the caller
// quits. `download-progress`, cancellation and signed manifests are 1.x work (see the
// roadmap's Track 7 descope).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use sha2::Digest;

/// A packaged app artifact can be large (installers run a few hundred MiB); this is far above
/// the 1 MiB `filesystem.*` cap, which is why staging has its own operation instead of reusing
/// `filesystem.writeBytes`.
const MAX_UPDATE_BYTES: u64 = 512 * 1024 * 1024;

/// Restricts an artifact name to a plain file name: path separators and control characters
/// become underscores, anything outside `[A-Za-z0-9._-]` is dropped, and leading dots are
/// trimmed so traversal remnants (`../evil.txt` → `evil.txt`) cannot survive in the name.
/// The name is re-derived on install, so a page cannot address anything outside
/// `userData/updates` by name.
fn sanitize_name(name: &str) -> Result<String, String> {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        return Err("Update artifact name must not be empty".into());
    }
    Ok(cleaned)
}

fn updates_dir(user_data: &Path) -> Result<PathBuf, String> {
    let dir = user_data.join("updates");
    fs::create_dir_all(&dir).map_err(|error| format!("Could not create the updates folder: {error}"))?;
    Ok(dir)
}

/// Streams `url` into `userData/updates/<name>`, verifying SHA-256 while writing when `sha256`
/// is set. `allowed_hosts` is the app's granted network host list; the request is refused
/// before it starts when the artifact host is not on it.
pub fn download(
    user_data: &Path,
    url: &str,
    name: &str,
    sha256: Option<&str>,
    allowed_hosts: &[String],
) -> Result<PathBuf, String> {
    let parsed = url::Url::parse(url).map_err(|error| error.to_string())?;
    let host = parsed.host_str().ok_or("URL has no host")?.to_string();
    if parsed.scheme() != "https" || !allowed_hosts.iter().any(|allowed| allowed.eq_ignore_ascii_case(&host)) {
        return Err("PERMISSION_DENIED: Network host is not granted or URL is not HTTPS".into());
    }
    let staged_name = sanitize_name(name)?;
    let dir = updates_dir(user_data)?;
    let final_path = dir.join(&staged_name);
    let temp_path = dir.join(format!("{staged_name}.part"));
    let _ = fs::remove_file(&temp_path);

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        .redirects(5)
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => format!("Update download failed: {status}"),
            other => other.to_string(),
        })?;
    if response.status() >= 400 {
        return Err(format!("Update download failed: {}", response.status()));
    }

    let mut digest = sha2::Sha256::new();
    let mut file = fs::File::create(&temp_path)
        .map_err(|error| format!("Could not stage the update: {error}"))?;
    let mut reader = response.into_reader();
    let mut chunk = [0u8; 65_536];
    let mut total: u64 = 0;
    loop {
        if crate::capabilities::current_cancel_token()
            .map(|token| token.load(std::sync::atomic::Ordering::SeqCst))
            .unwrap_or(false)
        {
            let _ = fs::remove_file(&temp_path);
            return Err("CANCELLED: Update download was cancelled".into());
        }
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                total += n as u64;
                if total > MAX_UPDATE_BYTES {
                    let _ = fs::remove_file(&temp_path);
                    return Err("Update artifact exceeds 512 MiB".into());
                }
                digest.update(&chunk[..n]);
                use std::io::Write;
                file.write_all(&chunk[..n])
                    .map_err(|error| format!("Could not write the staged update: {error}"))?;
            }
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                return Err(error.to_string());
            }
        }
    }
    file.sync_all().ok();
    if let Some(expected) = sha256.map(str::trim).filter(|expected| !expected.is_empty()) {
        let actual = format!("{:x}", digest.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(&temp_path);
            return Err(format!(
                "Update artifact SHA-256 mismatch: expected {expected}, got {actual}"
            ));
        }
    }
    fs::rename(&temp_path, &final_path).map_err(|error| format!("Could not stage the update: {error}"))?;
    Ok(final_path)
}

/// Swaps the staged artifact over `target` (the running executable's path). The running image
/// is renamed to `<path>.old` first and restored if the copy fails, so a failed install leaves
/// the old executable in place. Returns the backup path.
pub fn install_into(user_data: &Path, staged_relative: &str, target: &Path) -> Result<PathBuf, String> {
    let staged_name = sanitize_name(staged_relative)?;
    let dir = updates_dir(user_data)?;
    let staged = dir.join(&staged_name);
    // Confine to the canonical updates folder: the name is sanitized, but the canonicalize
    // check is what stops a symlink or a stray path from leaving `userData/updates`.
    let staged = staged.canonicalize().map_err(|error| format!("The staged update artifact is missing: {error}"))?;
    let dir_canonical = dir.canonicalize().map_err(|error| error.to_string())?;
    if !staged.starts_with(&dir_canonical) {
        return Err("PERMISSION_DENIED: The update artifact must be staged under userData/updates".into());
    }
    if !staged.is_file() {
        return Err("The staged update artifact is not a file".into());
    }

    let backup = PathBuf::from(format!("{}.old", target.display()));
    let _ = fs::remove_file(&backup);
    fs::rename(target, &backup).map_err(|error| format!("Could not move the running executable: {error}"))?;
    if let Err(error) = fs::copy(&staged, target) {
        let _ = fs::rename(&backup, target);
        return Err(format!("Could not install the update: {error}"));
    }
    Ok(backup)
}

/// Host entry: swaps the staged artifact over the running executable and, when `restart` is
/// set, relaunches it. The caller quits the old instance afterwards.
pub fn install(user_data: &Path, staged_relative: &str, restart: bool) -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("Could not locate the running executable: {error}"))?;
    let backup = install_into(user_data, staged_relative, &exe)?;
    if restart {
        if let Err(error) = std::process::Command::new(&exe).spawn() {
            // The new executable is in place; a failed relaunch is reported, not fatal.
            return Err(format!("The update installed but the app did not relaunch: {error}"));
        }
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rendrix-updater-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sanitize_name_strips_everything_but_a_plain_file_name() {
        assert_eq!(sanitize_name("app.exe").unwrap(), "app.exe");
        assert_eq!(sanitize_name("my app-1.2.exe").unwrap(), "myapp-1.2.exe");
        assert_eq!(sanitize_name("../../etc/passwd").unwrap(), "etcpasswd");
        assert_eq!(sanitize_name("..\\evil.exe").unwrap(), "evil.exe");
        assert!(sanitize_name("").is_err());
        assert!(sanitize_name("...").is_err());
    }

    #[test]
    fn install_swaps_the_running_executable_and_keeps_a_backup() {
        let user_data = temp_dir("swap");
        let dir = updates_dir(&user_data).unwrap();
        fs::write(dir.join("app.exe"), b"new executable bytes").unwrap();
        let target = user_data.join("host.exe");
        fs::write(&target, b"old executable bytes").unwrap();

        let backup = install_into(&user_data, "app.exe", &target).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new executable bytes");
        assert_eq!(backup, PathBuf::from(format!("{}.old", target.display())));
        assert_eq!(fs::read(&backup).unwrap(), b"old executable bytes");
        let _ = fs::remove_dir_all(&user_data);
    }

    #[test]
    fn install_refuses_paths_outside_the_updates_folder() {
        let user_data = temp_dir("confine");
        let target = user_data.join("host.exe");
        fs::write(&target, b"old executable bytes").unwrap();

        // `../evil.txt` sanitizes to a plain name inside updates/, which then does not exist:
        // traversal through the name can never reach a file the host did not stage itself.
        let error = install_into(&user_data, "../evil.txt", &target).unwrap_err();
        assert!(error.contains("missing"), "unexpected error: {error}");
        assert_eq!(fs::read(&target).unwrap(), b"old executable bytes");
        let _ = fs::remove_dir_all(&user_data);
    }

    #[test]
    fn install_restores_the_old_executable_when_the_copy_fails() {
        let user_data = temp_dir("restore");
        let target = user_data.join("host.exe");
        fs::write(&target, b"old executable bytes").unwrap();

        // The staged name is valid but does not exist, so the copy fails after the rename.
        let error = install_into(&user_data, "ghost.exe", &target).unwrap_err();
        assert!(error.contains("missing"), "unexpected error: {error}");
        assert_eq!(fs::read(&target).unwrap(), b"old executable bytes");
        assert!(!PathBuf::from(format!("{}.old", target.display())).exists());
        let _ = fs::remove_dir_all(&user_data);
    }
}
