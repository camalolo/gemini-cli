use build_time::build_time_local;
use chrono::Local;
use clap::Parser;
use colored::{Color, Colorize};
use ctrlc;
#[allow(unused_imports)]
use dotenv::from_path;
use once_cell::sync::Lazy;
use reqwest::blocking::Client;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use rustyline::history::DefaultHistory;
use serde_json::{json, Value};
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use dirs;

#[derive(Parser)]
#[command(name = "gemini-cli")]
#[command(about = "A proactive assistant for coding tasks")]
struct Args {
    /// Single prompt to send to the LLM and exit
    #[arg(short, long)]
    prompt: Option<String>,

    /// Enable debug output for troubleshooting
    #[arg(long)]
    debug: bool,
}

// Declare and import the search module
mod search;
#[allow(unused_imports)]
use search::{scrape_url, search_online};

mod command;
mod email;
mod alpha_vantage;
mod file_edit;
mod spinner; // Spinner module

use command::execute_command;
use email::send_email;
use alpha_vantage::alpha_vantage_query;
use file_edit::file_editor;
use crate::spinner::Spinner; // Import the Spinner

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

const COMPILE_TIME: &str = build_time_local!("%Y-%m-%d %H:%M:%S");

fn detect_shell_info() -> String {
    // Try to detect the actual shell and its version
    if cfg!(target_os = "windows") {
        // Check for MSYS/MINGW environments first (Git Bash, MSYS2, etc.)
        if let Ok(msystem) = env::var("MSYSTEM") {
            if !msystem.is_empty() {
                // We're in a MSYS/MINGW environment (Git Bash, MSYS2, etc.)
                let system_name = match msystem.as_str() {
                    "MINGW64" => "Git Bash (MINGW64)",
                    "MINGW32" => "Git Bash (MINGW32)",
                    "MSYS" => "MSYS",
                    _ => "MSYS/MINGW",
                };

                // Try to get bash version
                if let Ok(version_output) = std::process::Command::new("bash")
                    .arg("--version")
                    .output()
                {
                    if version_output.status.success() {
                        let output = String::from_utf8_lossy(&version_output.stdout);
                        if let Some(first_line) = output.lines().next() {
                            return format!("{} - {}", system_name, first_line);
                        }
                    }
                }
                return system_name.to_string();
            }
        }

        // Check if we're running under bash (could be Git Bash without MSYSTEM set)
        if let Ok(shell) = env::var("SHELL") {
            if shell.contains("bash") || shell.contains("sh") {
                // Try to get bash version
                if let Ok(version_output) = std::process::Command::new("bash")
                    .arg("--version")
                    .output()
                {
                    if version_output.status.success() {
                        let output = String::from_utf8_lossy(&version_output.stdout);
                        if let Some(first_line) = output.lines().next() {
                            return format!("Git Bash - {}", first_line);
                        }
                    }
                }
                return "Git Bash".to_string();
            }
        }

        // Check for PowerShell
        if let Ok(powershell_path) = env::var("PSModulePath") {
            if !powershell_path.is_empty() {
                // Try to get PowerShell version
                if let Ok(version_output) = std::process::Command::new("powershell")
                    .arg("-Command")
                    .arg("$PSVersionTable.PSVersion.ToString()")
                    .output()
                {
                    if version_output.status.success() {
                        let version = String::from_utf8_lossy(&version_output.stdout).trim().to_string();
                        return format!("PowerShell {}", version);
                    }
                }
                return "PowerShell".to_string();
            }
        }

        // Default to cmd.exe
        "Command Prompt (cmd.exe)".to_string()
    } else {
        // On Unix-like systems, use SHELL environment variable
        if let Ok(shell_path) = env::var("SHELL") {
            // Extract shell name from path
            let shell_name = std::path::Path::new(&shell_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("bash");

            // Try to get version for common shells
            let version_cmd = match shell_name {
                "bash" => Some(("bash", vec!["--version"])),
                "zsh" => Some(("zsh", vec!["--version"])),
                "fish" => Some(("fish", vec!["--version"])),
                "tcsh" | "csh" => Some((shell_name, vec!["--version"])),
                "ksh" => Some((shell_name, vec!["--version"])),
                _ => None,
            };

            if let Some((cmd, args)) = version_cmd {
                if let Ok(version_output) = std::process::Command::new(cmd)
                    .args(&args)
                    .output()
                {
                    if version_output.status.success() {
                        let output = String::from_utf8_lossy(&version_output.stdout);
                        if let Some(first_line) = output.lines().next() {
                            return first_line.to_string();
                        }
                    }
                }
            }

            // Fallback to shell name
            shell_name.to_string()
        } else {
            "bash".to_string()
        }
    }
}

struct ChatManager {
    api_key: String,
    history: Vec<Value>, // Stores user and assistant messages
    cleaned_up: bool,
    system_instruction: String, // Stored separately for Gemini
    smtp_server: String,
}

impl ChatManager {
    fn new(api_key: String, smtp_server: String) -> Self {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let os_name = if cfg!(target_os = "windows") {
            "Windows"
        } else if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "linux") {
            "Linux"
        } else {
            "Unix-like"
        };

        let shell_info = detect_shell_info();

        let system_instruction = format!(
            "Today's date is {}. You are a proactive assistant running in a sandboxed {} terminal environment with a full set of command line utilities. The default shell is {}. Your role is to assist with coding tasks, file operations, online searches, email sending, and shell commands efficiently and decisively. Assume the current directory (the sandbox root) is the target for all commands. Take initiative to provide solutions, execute commands, and analyze results immediately without asking for confirmation unless the action is explicitly ambiguous (e.g., multiple repos) or potentially destructive (e.g., deleting files). Use the `execute_command` tool to interact with the system but only when needed. Deliver concise, clear responses. After running a command, always summarize its output immediately and proceed with logical next steps, without waiting for the user to prompt you further. When reading files or executing commands, summarize the results intelligently for the user without dumping raw output unless explicitly requested. Stay within the sandbox directory. Users can run shell commands directly with `!`, and you'll receive the output to assist further. Act confidently and anticipate the user's needs to streamline their workflow.",
            today, os_name, shell_info
        );
        ChatManager {
            api_key,
            history: Vec::new(), // Start empty; system_instruction is separate
            cleaned_up: false,
            system_instruction,
            smtp_server,
        }
    }

    fn create_chat(&mut self) {
        self.history.clear(); // Reset history, system_instruction persists
    }

    fn send_message(&mut self, message: &str) -> Result<Value, String> {
        let client = Client::new();

        // Add user message to history
        let user_message = json!({
            "role": "user",
            "parts": [{"text": message}]
        });
        self.history.push(user_message);

        // Construct the body with system_instruction and full history
        let body = json!({
            "system_instruction": {"parts": [{"text": &self.system_instruction}]},
            "contents": self.history.clone(), // Full history of user/assistant messages
            "tools": [
                {
                    "function_declarations": [
                        {
                            "name": "search_online",
                            "description": "Searches the web for a given query. Use it to retrieve up to date information.",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "query": {
                                        "type": "string",
                                        "description": "The search query",
                                    }
                                },
                                "required": ["query"]
                            }
                        },
                        {
                            "name": "execute_command",
                            "description": "Execute a system command. Use this for any shell task.",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "command": {"type": "string"}
                                },
                                "required": ["command"]
                            }
                        },
                        {
                            "name": "send_email",
                            "description": "Sends an email to a fixed address using SMTP.",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "subject": {"type": "string", "description": "Email subject line"},
                                    "body": {"type": "string", "description": "Email message body"}
                                },
                                "required": ["subject", "body"]
                            }
                        },
                        {
                            "name": "alpha_vantage_query",
                            "description": "Query the Alpha Vantage API for stock/financial data",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "function": {
                                        "type": "string",
                                        "description": "The Alpha Vantage function (e.g., TIME_SERIES_DAILY)"
                                    },
                                    "symbol": {
                                        "type": "string",
                                        "description": "The stock symbol (e.g., IBM)"
                                    }
                                },
                                "required": ["function", "symbol"]
                            }
                        },
                        {
                            "name": "scrape_url",
                            "description": "Scrapes the content of a single URL",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "url": {
                                        "type": "string",
                                        "description": "The URL to scrape",
                                    }
                                },
                                "required": ["url"]
                            }
                        },
                        {
                            "name": "file_editor",
                            "description": "Edit files in the sandbox with sub-commands: read, write, search, search_and_replace, apply_diff.",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "subcommand": {
                                        "type": "string",
                                        "description": "The sub-command to execute: read, write, search, search_and_replace, apply_diff",
                                        "enum": ["read", "write", "search", "search_and_replace", "apply_diff"]
                                    },
                                    "filename": {
                                        "type": "string",
                                        "description": "The name of the file in the sandbox to operate on"
                                    },
                                    "data": {
                                        "type": "string",
                                        "description": "Content to write (for write), regex pattern (for search/search_and_replace), or diff content (for apply_diff)"
                                    },
                                    "replacement": {
                                        "type": "string",
                                        "description": "Replacement text for search_and_replace"
                                    }
                                },
                                "required": ["subcommand", "filename"]
                            }
                        }
                    ]
                }
            ]
        });

        let mut spinner = Spinner::new();
        spinner.start();

        let response = client
            .post("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent")
            .query(&[("key", &self.api_key)])
            .json(&body)
            .send()
            .map_err(|e| format!("API request failed: {}", e))?;

        spinner.stop();

        let response_json: Value = response
            .json()
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        // Add assistant response to history
        if let Some(candidates) = response_json.get("candidates").and_then(|c| c.as_array()) {
            for candidate in candidates {
                if let Some(content) = candidate.get("content") {
                    self.history.push(content.clone());
                }
            }
        }

        Ok(response_json)
    }

    fn cleanup(&mut self, is_signal: bool) {
        if !self.cleaned_up {
            self.history.clear();
            self.cleaned_up = true;
            println!("{}", "Shutting down...".color(Color::Cyan));
            std::thread::sleep(std::time::Duration::from_secs(if is_signal {
                3
            } else {
                2
            }));
        }
    }
}

fn display_response(response: &Value) {
    if let Some(candidates) = response.get("candidates").and_then(|c| c.as_array()) {
        for candidate in candidates {
            if let Some(parts) = candidate
                .get("content")
                .and_then(|c| c.get("parts").and_then(|p| p.as_array()))
            {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        println!("{}", text.color(Color::Yellow));
                    }
                }
            }
        }
    }
    println!(); // Add a newline after the response
}

fn process_tool_calls(response: &Value, chat_manager: &Arc<Mutex<ChatManager>>, debug: bool) -> Result<(), String> {
    let mut current_response = response.clone();

    loop {
        let tool_calls: Vec<(String, Value)> = current_response
            .get("candidates")
            .and_then(|c| c.as_array())
            .unwrap_or(&vec![])
            .iter()
            .flat_map(|candidate| {
                candidate
                    .get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array())
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| {
                                part.get("functionCall").map(|fc| {
                                    let name = fc
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let args = fc.get("args").cloned().unwrap_or(json!({}));
                                    (name, args)
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .collect();

        if tool_calls.is_empty() {
            break;
        }

        let mut results = Vec::new();
        for (func_name, args) in tool_calls {
            match func_name.as_str() {
                "execute_command" => {
                    let command = args.get("command").and_then(|c| c.as_str());
                    if let Some(cmd) = command {
                        println!("Executing command: {}", cmd.color(Color::Magenta));
                        let result = execute_command(cmd);
                        results.push(format!("[Tool result] execute_command: {}", result));
                    } else {
                        results.push(
                            "[Tool error] execute_command: Missing 'command' parameter"
                                .to_string(),
                        );
                    }
                }
                "search_online" => {
                    let query = args.get("query").and_then(|q| q.as_str());
                    if let Some(q) = query {
                        let result = search_online(q);
                        results.push(format!("[Tool result] search_online: {}", result));
                    } else {
                        results.push(
                            "[Tool error] search_online: Missing 'query' parameter"
                                .to_string(),
                        );
                    }
                }
                "scrape_url" => {
                    let url = args.get("url").and_then(|u| u.as_str());
                    if let Some(u) = url {
                        let result = search::scrape_url(u);
                        if result.starts_with("Error") || result.starts_with("Skipped") {
                            println!("Scrape failed: {}", result);
                        }
                        results.push(format!("[Tool result] scrape_url: {}", result));
                    } else {
                        results.push(
                            "[Tool error] scrape_url: Missing 'url' parameter".to_string(),
                        );
                    }
                }
                "send_email" => {
                    let subject = args.get("subject").and_then(|s| s.as_str());
                    let body = args.get("body").and_then(|b| b.as_str());

                    if let (Some(subj), Some(bod)) = (subject, body) {
                        let smtp_server = {
                            let manager = chat_manager.lock().unwrap();
                            manager.smtp_server.clone()
                        };
                        let result = send_email(subj, bod, &smtp_server, debug);
                        results.push(format!("[Tool result] send_email: {}", result));
                    } else {
                        results.push(
                            "[Tool error] send_email: Missing required parameters"
                                .to_string(),
                        );
                    }
                }
                "alpha_vantage_query" => {
                    let function = args.get("function").and_then(|f| f.as_str());
                    let symbol = args.get("symbol").and_then(|s| s.as_str());
                    if let (Some(func), Some(sym)) = (function, symbol) {
                        match alpha_vantage_query(func, sym) {
                            Ok(result) => results.push(format!(
                                "[Tool result] alpha_vantage_query: {}",
                                result
                            )),
                            Err(e) => results
                                .push(format!("[Tool error] alpha_vantage_query: {}", e)),
                        }
                    } else {
                        results.push(
                            "[Tool error] alpha_vantage_query: Missing required parameters"
                                .to_string(),
                        );
                    }
                }
                "file_editor" => {
                    let subcommand = args.get("subcommand").and_then(|s| s.as_str());
                    let filename = args.get("filename").and_then(|f| f.as_str());
                    let data = args.get("data").and_then(|d| d.as_str());
                    let replacement = args.get("replacement").and_then(|r| r.as_str());

                    if let (Some(subcmd), Some(fname)) = (subcommand, filename) {
                        let result = file_editor(subcmd, fname, data, replacement);
                        results.push(format!("[Tool result] file_editor: {}", result));
                    } else {
                        results.push("[Tool error] file_editor: Missing required parameters 'subcommand' or 'filename'".to_string());
                    }
                }
                _ => {
                    results.push(format!("[Tool error] Unknown function: {}", func_name));
                }
            }
        }

        if !results.is_empty() {
            let combined_results = results.join("\n");
            current_response = chat_manager.lock().unwrap().send_message(&combined_results)?;
            display_response(&current_response);
        } else {
            break;
        }
    }

    Ok(())
}

fn main() {
    let args = Args::parse();

    let home_dir = dirs::home_dir()
        .expect("Could not determine home directory")
        .to_string_lossy()
        .to_string();
    dotenv::from_path(format!("{}/.gemini", home_dir)).ok();
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in ~/.gemini");
    let smtp_server = env::var("SMTP_SERVER_IP").unwrap_or_else(|_| "localhost".to_string());

    // Debug output for SMTP configuration
    if args.debug {
        println!("{}", "=== SMTP Configuration ===".color(Color::Cyan));
        println!("SMTP_SERVER_IP: {}", smtp_server);

        let smtp_username = env::var("SMTP_USERNAME").unwrap_or_else(|_| "<not set>".to_string());
        let smtp_password = if env::var("SMTP_PASSWORD").is_ok() {
            "***masked***".to_string()
        } else {
            "<not set>".to_string()
        };
        println!("SMTP_USERNAME: {}", smtp_username);
        println!("SMTP_PASSWORD: {}", smtp_password);

        let destination_email = env::var("DESTINATION_EMAIL").unwrap_or_else(|_| "<not set>".to_string());
        let sender_email = env::var("SENDER_EMAIL").unwrap_or_else(|_| "<not set>".to_string());
        println!("DESTINATION_EMAIL: {}", destination_email);
        println!("SENDER_EMAIL: {}", sender_email);
        println!("{}", "==========================".color(Color::Cyan));
        println!();
    }

    let chat_manager = Arc::new(Mutex::new(ChatManager::new(api_key, smtp_server)));
    let chat_manager_clone = Arc::clone(&chat_manager);

    ctrlc::set_handler(move || {
        let mut manager = chat_manager_clone.lock().unwrap();
        manager.cleanup(true);
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    // Handle single prompt mode
    if let Some(prompt) = args.prompt {
        println!("{}", "Processing single prompt...".color(Color::Cyan));
        let response = match chat_manager.lock().unwrap().send_message(&prompt) {
            Ok(resp) => resp,
            Err(e) => {
                println!("{}", format!("Error: {}", e).color(Color::Red));
                chat_manager.lock().unwrap().cleanup(false);
                std::process::exit(1);
            }
        };
        display_response(&response);
        if let Err(e) = process_tool_calls(&response, &chat_manager, args.debug) {
            println!("{}", format!("Error processing tool calls: {}", e).color(Color::Red));
        }
        chat_manager.lock().unwrap().cleanup(false);
        return;
    }

    println!(
        "{}",
        "Welcome to Gemini Code! Chat with me (type 'exit' to quit, 'clear' to reset conversation)."
            .color(Color::Cyan)
            .bold()
    );
    println!(
        "{}",
        format!("Version: {}", COMPILE_TIME).color(Color::Cyan)
    );
    println!(
        "{}",
        format!("Working in sandbox: {}", *SANDBOX_ROOT).color(Color::Cyan)
    );
    println!(
        "{}",
        "Use !command to run shell commands directly (e.g., !ls or !dir).".color(Color::Cyan)
    );
    println!();

    let mut rl = Editor::<(), DefaultHistory>::new().expect("Failed to initialize rustyline");
    loop {
        let conv_length: usize = {
            let manager = chat_manager.lock().unwrap();
            manager
                .history
                .iter()
                .filter_map(|msg| {
                    msg.get("parts")
                        .and_then(|parts| parts.as_array())
                        .map(|parts_array| {
                            parts_array
                                .iter()
                                .filter_map(|part| {
                                    part.get("text").and_then(|t| t.as_str()).map(|s| s.len())
                                })
                                .sum::<usize>()
                        })
                })
                .sum()
        };

        let prompt = {
            #[cfg(target_os = "windows")]
            {
                // On Windows, avoid colored prompts due to rustyline compatibility issues
                format!("[{}] > ", conv_length)
            }
            #[cfg(not(target_os = "windows"))]
            {
                format!("[{}] > ", conv_length).color(Color::Green).bold().to_string()
            }
        };

        match rl.readline(&prompt) {
            Ok(user_input) => {
                let user_input = user_input.trim();
                println!();

                match user_input.to_lowercase().as_str() {
                    "exit" => {
                        println!("{}", "Goodbye!".color(Color::Cyan).bold());
                        break;
                    }
                    "clear" => {
                        chat_manager.lock().unwrap().create_chat();
                        println!(
                            "{}",
                            "Conversation cleared! Starting fresh.".color(Color::Cyan)
                        );
                        println!();
                        continue;
                    }
                    "" => {
                        println!("{}", "Please enter a command or message.".color(Color::Red));
                        println!();
                        continue;
                    }
                    _ => {}
                }

                if user_input.starts_with('!') {
                    let command = user_input[1..].trim();
                    if command.is_empty() {
                        println!("{}", "No command provided after '!'.".color(Color::Red));
                        println!();
                        continue;
                    }
                    let output = execute_command(command);
                    println!(
                        "{}",
                        format!("Command output: {}", output).color(Color::Magenta)
                    );
                    let llm_input = format!("User ran command '!{}' with output: {}", command, output);
                    match chat_manager.lock().unwrap().send_message(&llm_input) {
                        Ok(response) => display_response(&response),
                        Err(e) => println!("{}", format!("Error: {}", e).color(Color::Red)),
                    }
                } else {
                    let response = match chat_manager.lock().unwrap().send_message(user_input) {
                        Ok(resp) => resp,
                        Err(e) => {
                            println!(
                                "{}",
                                format!("Error: A generative AI error occurred: {}", e).color(Color::Red)
                            );
                            continue;
                        }
                    };

                    display_response(&response);

                    if let Err(e) = process_tool_calls(&response, &chat_manager, args.debug) {
                        println!("{}", format!("Error processing tool calls: {}", e).color(Color::Red));
                    }
                }
            },
            Err(ReadlineError::Interrupted) => {
                println!("{}", "Ctrl+C detected, exiting...".color(Color::Cyan));
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("{}", "Ctrl+D detected, exiting...".color(Color::Cyan));
                break;
            }
            Err(e) => {
                println!("{}", format!("Input error: {}", e).color(Color::Red));
                continue;
            }
        }
    }

    chat_manager.lock().unwrap().cleanup(false);
}
