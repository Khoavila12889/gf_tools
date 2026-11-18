//! First-logon application setup explicitly requested by the owner.
use std::path::Path;

pub const FIRST_LOGON_COMMAND: &str = r#"cmd.exe /c start "" /b powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "%ProgramData%\QSU\PostInstall.ps1""#;

/// Embed the worker in both installation executables; no external file can be
/// forgotten when preparing a client. This writes only to the offline target.
pub fn write_to_windows(root: &Path) -> std::io::Result<()> {
    let destination = root.join("ProgramData").join("QSU");
    std::fs::create_dir_all(&destination)?;
    std::fs::write(destination.join("Telegram.ps1"), include_bytes!("telegram.ps1"))?;
    std::fs::write(destination.join("PostInstall.ps1"), include_bytes!("post_install.ps1"))
}

/// Keep the bot credential external to binaries. SID-based ACLs also work in PE.
pub fn copy_telegram_config(source: &Path, destination: &Path) -> std::io::Result<bool> {
    let source = source.join("Telegram.json");
    if !source.is_file() { return Ok(false); }
    std::fs::create_dir_all(destination)?;
    let target = destination.join("Telegram.json");
    // Restrict the destination before writing any credential bytes.
    if !target.exists() { std::fs::write(&target, b"")?; }
    let path = target.to_string_lossy();
    let output = crate::cmd::run_capture("icacls.exe", &[&path, "/inheritance:r", "/grant:r", "*S-1-5-18:F", "*S-1-5-32-544:F"])?;
    if output.code != Some(0) { return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied,"Cannot protect Telegram configuration")); }
    let bytes = std::fs::read(source)?;
    std::fs::write(target, bytes)?;
    Ok(true)
}

pub fn copy_host_telegram_config(destination: &Path) -> std::io::Result<bool> {
    let exe = std::env::current_exe()?;
    if let Some(parent) = exe.parent() {
        if copy_telegram_config(parent, destination)? { return Ok(true); }
    }
    if let Some(data) = std::env::var_os("ProgramData") {
        return copy_telegram_config(&std::path::PathBuf::from(data).join("QSU"), destination);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn stages_worker_into_offline_windows_without_running_it() {
        let root = std::env::temp_dir().join(format!("qsu-post-test-{}",std::process::id()));
        super::write_to_windows(&root).unwrap();
        assert_eq!(std::fs::read(root.join("ProgramData/QSU/PostInstall.ps1")).unwrap(),include_bytes!("post_install.ps1"));
        std::fs::remove_file(root.join("ProgramData/QSU/PostInstall.ps1")).unwrap();
        std::fs::remove_file(root.join("ProgramData/QSU/Telegram.ps1")).unwrap();
        std::fs::remove_dir(root.join("ProgramData/QSU")).unwrap();
        std::fs::remove_dir(root.join("ProgramData")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
