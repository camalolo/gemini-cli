use chrono::Local;
use std::env;
use std::io::Write;
use std::process::Command;

pub fn send_email(subject: &str, body: &str) -> String {
    let recipient =
        env::var("DESTINATION_EMAIL").expect("DESTINATION_EMAIL not found in ~/.gemini");

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
