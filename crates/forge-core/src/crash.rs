use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::panic;
use std::path::PathBuf;
use std::process::Command;
use std::backtrace::Backtrace;

/// Returns the path to the crash log file: ~/.cache/forge/crash.log
pub fn crash_log_path() -> PathBuf {
    if let Ok(cache_home) = env::var("XDG_CACHE_HOME") {
        if !cache_home.is_empty() {
            let mut path = PathBuf::from(cache_home);
            path.push("forge");
            path.push("crash.log");
            return path;
        }
    }
    
    // Fallback to ~/.cache
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            let mut path = PathBuf::from(home);
            path.push(".cache");
            path.push("forge");
            path.push("crash.log");
            return path;
        }
    }

    PathBuf::from("/tmp/forge_crash.log")
}

/// Installs the global panic hook.
/// On panic, writes the error message and backtrace to crash_log_path().
/// Then attempts to send a desktop notification via notify-send.
/// Finally, re-panics so the process terminates with the correct exit code.
pub fn install_panic_handler() {
    panic::set_hook(Box::new(|panic_info| {
        let path = crash_log_path();
        
        // 1. Ensure the parent directory of crash_log_path() exists.
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // 3a. Format the panic info (location, message).
        let location = panic_info.location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());
        
        let payload = panic_info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "Box<dyn Any>".to_string()
        };

        let formatted_info = format!("Panic occurred at {}: {}", location, message);

        // 3b. Capture RUST_BACKTRACE=1 output using std::backtrace::Backtrace::capture().
        let backtrace = Backtrace::force_capture();
        
        // 3c. Write formatted output to the crash log file (open in append mode).
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{}\n{}", formatted_info, backtrace);
            let _ = writeln!(file, "----------------------------------------");
        }

        // 3d. Attempt std::process::Command::new("notify-send") with a friendly message.
        // This must be non-blocking and its failure must be ignored.
        let msg = format!("Forge crashed! Log written to: {}", path.display());
        let _ = Command::new("notify-send")
            .arg("Forge Error")
            .arg(&msg)
            .spawn();

        // 3e. Call eprintln! to also print to stderr.
        eprintln!("{}", formatted_info);
        eprintln!("{}", backtrace);
    }));
}
