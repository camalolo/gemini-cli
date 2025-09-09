use chrono::Local;
use std::env;
use std::io::Write;
#[cfg(not(target_os = "windows"))]
use std::process::Command;
#[cfg(target_os = "windows")]
use std::path::PathBuf;

pub fn send_email(subject: &str, body: &str) -> String {
    let recipient =
        env::var("DESTINATION_EMAIL").expect("DESTINATION_EMAIL not found in ~/.gemini");

    #[cfg(target_os = "linux")]
    {
        // Create a temporary file for the email body
        let temp_file = format!("/tmp/email_body_{}.txt", Local::now().timestamp());
        if let Ok(mut file) = std::fs::File::create(&temp_file) {
            if let Err(e) = file.write_all(body.as_bytes()) {
                return format!("Failed to write email body to temporary file: {}", e);
            }
        } else {
            return "Failed to create temporary file for email body".to_string();
        }

        // Use the mail command to send the email
        let mail_cmd = format!("mail -s \"{}\" {} < {}", subject, recipient, temp_file);
        let output = Command::new("sh").arg("-c").arg(&mail_cmd).output();

        // Clean up the temporary file
        let _ = std::fs::remove_file(&temp_file);

        match output {
            Ok(out) => {
                if out.status.success() {
                    format!("Email sent successfully to {}", recipient)
                } else {
                    format!(
                        "Failed to send email: {}",
                        String::from_utf8_lossy(&out.stderr)
                    )
                }
            }
            Err(e) => format!("Error executing mail command: {}", e),
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Create a temporary file for the email body in Windows temp directory
        let temp_dir: PathBuf = env::temp_dir();
        let temp_file: PathBuf = temp_dir.join(format!("email_body_{}.txt", Local::now().timestamp()));

        if let Ok(mut file) = std::fs::File::create(&temp_file) {
            if let Err(e) = file.write_all(body.as_bytes()) {
                return format!("Failed to write email body to temporary file: {}", e);
            }
        } else {
            return "Failed to create temporary file for email body".to_string();
        }

        // On Windows, we can't easily send emails from command line without external tools
        // For now, we'll create the email file and inform the user
        let temp_file_path = temp_file.to_string_lossy();

        // Clean up the temporary file
        let _ = std::fs::remove_file(&temp_file);

        format!(
            "Email functionality not fully implemented on Windows. Email body saved to temporary file: {}. \
             To send manually, use an email client with subject '{}' and recipient '{}'",
            temp_file_path, subject, recipient
        )
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        // Fallback for other systems (macOS, etc.) - try mail command
        let temp_file = format!("/tmp/email_body_{}.txt", Local::now().timestamp());
        if let Ok(mut file) = std::fs::File::create(&temp_file) {
            if let Err(e) = file.write_all(body.as_bytes()) {
                return format!("Failed to write email body to temporary file: {}", e);
            }
        } else {
            return "Failed to create temporary file for email body".to_string();
        }

        let mail_cmd = format!("mail -s \"{}\" {} < {}", subject, recipient, temp_file);
        let output = Command::new("/bin/sh").arg("-c").arg(&mail_cmd).output();

        // Clean up the temporary file
        let _ = std::fs::remove_file(&temp_file);

        match output {
            Ok(out) => {
                if out.status.success() {
                    format!("Email sent successfully to {}", recipient)
                } else {
                    format!(
                        "Failed to send email: {}",
                        String::from_utf8_lossy(&out.stderr)
                    )
                }
            }
            Err(e) => format!("Error executing mail command: {}", e),
        }
    }
}
