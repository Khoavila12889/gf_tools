#![windows_subsystem = "windows"]
use std::{env, fs, path::PathBuf, process::Command, time::{SystemTime, UNIX_EPOCH}};

include!("payload.rs");

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(window: *mut core::ffi::c_void, text: *const u16, title: *const u16, flags: u32) -> i32;
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().collect();
    let extract_only = args.len() == 3 && args[1] == "--extract-to";
    let root = if extract_only {
        PathBuf::from(&args[2])
    } else {
        env::temp_dir().join(format!("UtilityRebuild-232f4db-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()))
    };
    // A fresh directory prevents overwriting an existing payload or following its files.
    fs::create_dir(&root)?;
    for (name, bytes) in PAYLOAD {
        let path = root.join(name);
        fs::create_dir_all(path.parent().ok_or("Invalid payload path")?)?;
        fs::write(path, bytes)?;
    }
    if !extract_only {
        Command::new(root.join("launcher.exe")).current_dir(&root).spawn()?.wait()?;
        // Modules are independent elevated processes and can outlive the launcher.
        // Keep the payload available for them and for installation staging.
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        let text: Vec<u16> = format!("Khong mo duoc Utility Rebuild:\n{error}").encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = "Utility Rebuild".encode_utf16().chain(Some(0)).collect();
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10); }
        std::process::exit(1);
    }
}
