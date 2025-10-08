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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    match child {
        Ok(mut child_proc) => {
            let stdout = child_proc.stdout.take().unwrap();
            let stderr = child_proc.stderr.take().unwrap();

            if let Some(child_stdin) = child_proc.stdin.take() {
                // Start a thread to forward input from parent stdin to child stdin
                let input_handle = thread::spawn(move || {
                    let mut child_stdin = child_stdin;
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

                // Start threads to read stdout and stderr, print to terminal, and collect
                let stdout_handle = {
                    let mut stdout = stdout;
                    thread::spawn(move || {
                        let mut buf = Vec::new();
                        let mut temp = [0u8; 1024];
                        loop {
                            match stdout.read(&mut temp) {
                                Ok(0) => break,
                                Ok(n) => {
                                    io::stdout().write_all(&temp[..n]).ok();
                                    io::stdout().flush().ok();
                                    buf.extend_from_slice(&temp[..n]);
                                }
                                Err(_) => break,
                            }
                        }
                        buf
                    })
                };

                let stderr_handle = {
                    let mut stderr = stderr;
                    thread::spawn(move || {
                        let mut buf = Vec::new();
                        let mut temp = [0u8; 1024];
                        loop {
                            match stderr.read(&mut temp) {
                                Ok(0) => break,
                                Ok(n) => {
                                    io::stderr().write_all(&temp[..n]).ok();
                                    io::stderr().flush().ok();
                                    buf.extend_from_slice(&temp[..n]);
                                }
                                Err(_) => break,
                            }
                        }
                        buf
                    })
                };

                let status = child_proc.wait();
                input_handle.join().ok();

                let stdout_buf = stdout_handle.join().unwrap_or_default();
                let stderr_buf = stderr_handle.join().unwrap_or_default();

                match status {
                    Ok(_) => {
                        let stdout_str = String::from_utf8_lossy(&stdout_buf);
                        let stderr_str = String::from_utf8_lossy(&stderr_buf);

                        let output = if stdout_str.is_empty() && stderr_str.is_empty() {
                            "Command executed (no output)".to_string()
                        } else {
                            format!("{}{}", stdout_str, stderr_str)
                        };
                        output
                    }
                    Err(e) => format!("Error waiting for command '{}': {:?}", command, e),
                }
            } else {
                // No stdin pipe, just wait
                let status = child_proc.wait();
                match status {
                    Ok(_) => {
                        // Read stdout and stderr
                        let mut stdout_buf = Vec::new();
                        let mut stderr_buf = Vec::new();
                        let mut stdout = stdout;
                        let mut stderr = stderr;
                        stdout.read_to_end(&mut stdout_buf).ok();
                        stderr.read_to_end(&mut stderr_buf).ok();

                        let stdout_str = String::from_utf8_lossy(&stdout_buf);
                        let stderr_str = String::from_utf8_lossy(&stderr_buf);

                        let output = if stdout_str.is_empty() && stderr_str.is_empty() {
                            "Command executed (no output)".to_string()
                        } else {
                            format!("{}{}", stdout_str, stderr_str)
                        };
                        output
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
        let args = vec![
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

