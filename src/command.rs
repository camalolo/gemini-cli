use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::process::Command;
use std::str;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

static SANDBOX_ROOT: Lazy<String> = Lazy::new(|| {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string()
});

pub fn execute_command(command: &str) -> String {
    if command.trim().is_empty() {
        return "Error: No command provided".to_string();
    }

    #[cfg(target_os = "linux")]
    {
        // bubblewrap: mount / as read-only and only the SANDBOX_ROOT as read-write
        let output = Command::new("bwrap")
            .args(&[
                // Mount the entire filesystem as read-only
                "--ro-bind", "/", "/",
                // Bind mount the current directory as read-write
                "--bind", &*SANDBOX_ROOT, &*SANDBOX_ROOT,
                // Set up necessary pseudo filesystems
                "--dev", "/dev",
                "--proc", "/proc",
                // Optionally, unshare additional namespaces if needed (e.g., network)
                // Run the command in a shell
                "/bin/sh", "-c", command,
            ])
            .current_dir(&*SANDBOX_ROOT)
            .output();

        match output {
            Ok(out) => {
                if out.status.success() {
                    str::from_utf8(&out.stdout).unwrap_or("").to_string()
                } else {
                    let stderr = str::from_utf8(&out.stderr).unwrap_or("Unknown error");
                    format!("Error: {}", stderr)
                }
            }
            Err(e) => format!("Error executing '{}': {:?}", command, e),
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: Execute command in sandbox directory with restricted environment
        let output = Command::new("cmd")
            .args(&["/C", command])
            .current_dir(&*SANDBOX_ROOT)
            // On Windows, we restrict the working directory but don't have bwrap-level isolation
            .output();

        match output {
            Ok(out) => {
                if out.status.success() {
                    str::from_utf8(&out.stdout).unwrap_or("").to_string()
                } else {
                    let stderr = str::from_utf8(&out.stderr).unwrap_or("Unknown error");
                    format!("Error: {}", stderr)
                }
            }
            Err(e) => format!("Error executing '{}': {:?}", command, e),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        // Fallback for other Unix-like systems (macOS, etc.)
        let shell = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" };
        let output = Command::new(shell)
            .arg("-c")
            .arg(command)
            .current_dir(&*SANDBOX_ROOT)
            .output();

        match output {
            Ok(out) => {
                if out.status.success() {
                    str::from_utf8(&out.stdout).unwrap_or("").to_string()
                } else {
                    let stderr = str::from_utf8(&out.stderr).unwrap_or("Unknown error");
                    format!("Error: {}", stderr)
                }
            }
            Err(e) => format!("Error executing '{}': {:?}", command, e),
        }
    }
}

