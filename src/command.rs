use once_cell::sync::Lazy;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str;
use std::thread;

static SANDBOX_ROOT: Lazy<String> = Lazy::new(|| {
    let path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string();

    // On Windows, canonicalize() adds \\?\ prefix, remove it for display
    #[cfg(target_os = "windows")]
    {
        if path.starts_with("\\\\?\\") {
            path[4..].to_string()
        } else {
            path
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        path
    }
});

pub fn execute_command(command: &str) -> String {
    if command.trim().is_empty() {
        return "Error: No command provided".to_string();
    }

    let (program, args) = get_command_parts(command);

    let child = Command::new(&program)
        .args(&args)
        .current_dir(&*SANDBOX_ROOT)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn();

    match child {
        Ok(mut child_proc) => {
            if let Some(mut child_stdin) = child_proc.stdin.take() {
                // Start a thread to forward input from parent stdin to child stdin
                let handle = thread::spawn(move || {
                    let mut buffer = [0u8; 1024];
                    loop {
                        match io::stdin().read(&mut buffer) {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                if child_stdin.write_all(&buffer[..n]).is_err() {
                                    break; // Child stdin closed
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });

                let status = child_proc.wait();
                // The thread will stop when child_stdin is closed or on error
                handle.join().ok();

                match status {
                    Ok(status) => {
                        if status.success() {
                            "Command executed successfully".to_string()
                        } else {
                            format!("Command completed with exit code: {}", status.code().unwrap_or(-1))
                        }
                    }
                    Err(e) => format!("Error waiting for command '{}': {:?}", command, e),
                }
            } else {
                // No stdin pipe, just wait
                match child_proc.wait() {
                    Ok(status) => {
                        if status.success() {
                            "Command executed successfully".to_string()
                        } else {
                            format!("Command completed with exit code: {}", status.code().unwrap_or(-1))
                        }
                    }
                    Err(e) => format!("Error waiting for command '{}': {:?}", command, e),
                }
            }
        }
        Err(e) => format!("Error spawning command '{}': {:?}", command, e),
    }
}

fn get_command_parts(command: &str) -> (String, Vec<String>) {
    #[cfg(target_os = "linux")]
    {
        // Use bwrap
        let mut args = vec![
            "--ro-bind".to_string(), "/".to_string(), "/".to_string(),
            "--bind".to_string(), SANDBOX_ROOT.clone(), SANDBOX_ROOT.clone(),
            "--dev".to_string(), "/dev".to_string(),
            "--proc".to_string(), "/proc".to_string(),
            "/bin/sh".to_string(), "-c".to_string(), command.to_string(),
        ];
        ("bwrap".to_string(), args)
    }

    #[cfg(target_os = "windows")]
    {
        // Use cmd
        ("cmd".to_string(), vec!["/C".to_string(), command.to_string()])
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        // Use shell
        let shell = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" };
        (shell.to_string(), vec!["-c".to_string(), command.to_string()])
    }
}

