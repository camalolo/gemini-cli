use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::process::{Command, Child, Stdio};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::sync::Mutex;
use std::thread;

struct Sandbox {
    child: Option<Child>,
    name: String,
}

static SANDBOX: Lazy<Mutex<Sandbox>> = Lazy::new(|| {
    let sandbox_name = format!(
        "sandbox_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    Mutex::new(Sandbox {
        child: None,
        name: sandbox_name,
    })
});

static SANDBOX_ROOT: Lazy<String> = Lazy::new(|| {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string()
});

// Start the sandbox and wait for it to be ready
fn start_sandbox() -> Result<(), String> {
    let mut sandbox = SANDBOX.lock().unwrap();
    if sandbox.child.is_none() {
        eprintln!("Starting sandbox: {}", sandbox.name);
        eprintln!("Sandbox root: {}", *SANDBOX_ROOT);

        let child = Command::new("firejail")
            .args([
                "--quiet",
                "--noprofile",
                &format!("--whitelist={}", *SANDBOX_ROOT),
                "--caps.drop=all",
                "--seccomp",
                "--noroot",
                "--private-tmp",
                "--tmpfs=/tmp",
                &format!("--name={}", sandbox.name),
                "--",
                "/bin/sh",
            ])
            .current_dir(&*SANDBOX_ROOT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start sandbox: {:?}", e))?;

        sandbox.child = Some(child);

        // Wait until the sandbox is listed as active
        for _ in 0..10 { // Retry up to 10 times (1 second total)
            let output = Command::new("firejail")
                .arg("--list")
                .output()
                .map_err(|e| format!("Failed to check sandbox list: {:?}", e))?;
            let list = String::from_utf8_lossy(&output.stdout);
            if list.contains(&sandbox.name) {
                eprintln!("Sandbox {} is ready", sandbox.name);
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100)); // Wait 100ms between checks
        }
        return Err(format!("Sandbox {} failed to initialize", sandbox.name));
    }
    Ok(())
}

pub fn execute_command(command: &str) -> String {
    if command.trim().is_empty() {
        return "Error: No command provided".to_string();
    }

    if let Err(e) = start_sandbox() {
        return e;
    }

    let sandbox = SANDBOX.lock().unwrap();
    let sandbox_name = &sandbox.name;

    let output = Command::new("firejail")
        .args([
            "--quiet",
            &format!("--join={}", sandbox_name),
            "--",
            "/bin/sh",
            "-c",
            command,
        ])
        .current_dir(&*SANDBOX_ROOT)
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                String::from_utf8_lossy(&out.stdout).to_string()
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                format!("Error: {}", stderr)
            }
        }
        Err(e) => format!("Error executing '{}': {:?}", command, e),
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            eprintln!("Shutting down sandbox: {}", self.name);
            let _ = child.kill();
            let _ = child.wait();
            let _ = Command::new("firejail")
                .args(["--shutdown", &self.name])
                .output();
        }
    }
}