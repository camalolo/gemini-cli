use build_time::build_time_local;
use chrono::Local;
use colored::{Color, Colorize};
use ctrlc;
#[allow(unused_imports)]
use dotenv::from_path;
use landlock::{AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr};
use once_cell::sync::Lazy;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::env;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

// Declare and import the search module
mod search;
#[allow(unused_imports)]
use search::{scrape_url, search_online};

static SANDBOX_ROOT: Lazy<String> = Lazy::new(|| {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string()
});

const COMPILE_TIME: &str = build_time_local!("%Y-%m-%d %H:%M:%S");

struct ChatManager {
    api_key: String,
    history: Vec<Value>, // Stores user and assistant messages
    cleaned_up: bool,
    system_instruction: String, // Stored separately for Gemini
}

impl ChatManager {
    fn new(api_key: String) -> Self {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let system_instruction = format!(
            "Today's date is {}. You are a proactive assistant running in a sandboxed Linux terminal environment with a full set of command line utilities. Your role is to assist with coding tasks, file operations, online searches, email sending, and shell commands efficiently and decisively. Assume the current directory (the sandbox root) is the target for all commands. Take initiative to provide solutions, execute commands, and analyze results immediately without asking for confirmation unless the action is explicitly ambiguous (e.g., multiple repos) or potentially destructive (e.g., deleting files). Use the `execute_command` tool to interact with the system when needed, and deliver concise, clear responses. After running a command, always summarize its output immediately and proceed with logical next steps, without waiting for the user to prompt you further. When reading files or executing commands, summarize the results intelligently for the user without dumping raw output unless explicitly requested. Stay within the sandbox directory. Users can run shell commands directly with `!`, and you'll receive the output to assist further. Act confidently and anticipate the user's needs to streamline their workflow.",
            today
        );
        ChatManager {
            api_key,
            history: Vec::new(), // Start empty; system_instruction is separate
            cleaned_up: false,
            system_instruction,
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
                            "description": "Sends an email to a fixed address using the local mail system.",
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
                        }
                    ]
                }
            ]
        });

        let response = client
            .post("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash-001:generateContent")
            .query(&[("key", &self.api_key)])
            .json(&body)
            .send()
            .map_err(|e| format!("API request failed: {}", e))?;

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

fn execute_command(command: &str, _skip_confirm: bool) -> String {
    eprintln!("Sandbox root: {}", *SANDBOX_ROOT);

    let mut parts = command.split_whitespace();
    let cmd_name = match parts.next() {
        Some(name) => name,
        None => return "Error: No command provided".to_string(),
    };
    let args: Vec<&str> = parts.collect();

    if cmd_name == "cd" {
        return "Error: 'cd' is not supported in this sandboxed environment".to_string();
    }

    let mut cmd = Command::new(cmd_name);
    cmd.args(&args).current_dir(&*SANDBOX_ROOT);

    unsafe {
        cmd.pre_exec(|| {
            let ruleset = Ruleset::default()
                .handle_access(
                    AccessFs::Execute
                        | AccessFs::ReadFile
                        | AccessFs::WriteFile
                        | AccessFs::ReadDir,
                )
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Ruleset handle_access failed: {}", e),
                    )
                })?;

            let created_ruleset = ruleset.create().map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Ruleset create failed: {}", e),
                )
            })?;

            // Rule for SANDBOX_ROOT (full access)
            let root_fd = PathFd::new(&*SANDBOX_ROOT).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("PathFd for sandbox root failed: {}", e),
                )
            })?;
            let root_rule = PathBeneath::new(
                root_fd,
                AccessFs::Execute | AccessFs::ReadFile | AccessFs::WriteFile | AccessFs::ReadDir,
            );

            // Rule for /bin
            let bin_fd = PathFd::new("/bin").map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("PathFd for /bin failed: {}", e),
                )
            })?;
            let bin_rule = PathBeneath::new(bin_fd, AccessFs::Execute | AccessFs::ReadFile);

            // Rule for /usr/bin (e.g., pwd)
            let usr_bin_fd = PathFd::new("/usr/bin").map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("PathFd for /usr/bin failed: {}", e),
                )
            })?;
            let usr_bin_rule = PathBeneath::new(usr_bin_fd, AccessFs::Execute | AccessFs::ReadFile);

            // Rule for /lib
            let lib_fd = PathFd::new("/lib").map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("PathFd for /lib failed: {}", e),
                )
            })?;
            let lib_rule = PathBeneath::new(lib_fd, AccessFs::Execute | AccessFs::ReadFile);

            // Rule for /usr/lib
            let usr_lib_fd = PathFd::new("/usr/lib").map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("PathFd for /usr/lib failed: {}", e),
                )
            })?;
            let usr_lib_rule = PathBeneath::new(usr_lib_fd, AccessFs::Execute | AccessFs::ReadFile);

            // Rule for /lib64 (common on 64-bit systems)
            let lib64_fd = PathFd::new("/lib64").map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("PathFd for /lib64 failed: {}", e),
                )
            })?;
            let lib64_rule = PathBeneath::new(lib64_fd, AccessFs::Execute | AccessFs::ReadFile);

            // Rule for /proc (for pwd and others)
            let proc_fd = PathFd::new("/proc").map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("PathFd for /proc failed: {}", e),
                )
            })?;
            let proc_rule = PathBeneath::new(proc_fd, AccessFs::ReadFile);

            eprintln!(
                "Adding rules for: {}, /bin, /usr/bin, /lib, /usr/lib, /lib64, /proc",
                *SANDBOX_ROOT
            );

            created_ruleset
                .add_rule(root_rule)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Add sandbox root rule failed: {}", e),
                    )
                })?
                .add_rule(bin_rule)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Add /bin rule failed: {}", e),
                    )
                })?
                .add_rule(usr_bin_rule)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Add /usr/bin rule failed: {}", e),
                    )
                })?
                .add_rule(lib_rule)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Add /lib rule failed: {}", e),
                    )
                })?
                .add_rule(usr_lib_rule)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Add /usr/lib rule failed: {}", e),
                    )
                })?
                .add_rule(lib64_rule)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Add /lib64 rule failed: {}", e),
                    )
                })?
                .add_rule(proc_rule)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Add /proc rule failed: {}", e),
                    )
                })?
                .restrict_self()
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Restrict self failed: {}", e),
                    )
                })?;

            Ok(())
        });
    }

    cmd.output()
        .map(|out| {
            if out.status.success() {
                String::from_utf8_lossy(&out.stdout).to_string()
            } else {
                String::from_utf8_lossy(&out.stderr).to_string()
            }
        })
        .unwrap_or_else(|e| format!("Error executing '{}': {:?}", command, e))
}

fn send_email(subject: &str, body: &str) -> String {
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

fn alpha_vantage_query(function: &str, symbol: &str) -> Result<String, String> {
    let api_key =
        env::var("ALPHA_VANTAGE_API_KEY").expect("ALPHA_VANTAGE_API_KEY not found in ~/.gemini");
    let client = Client::new();

    let url = format!(
        "https://www.alphavantage.co/query?function={}&symbol={}&apikey={}",
        function, symbol, api_key
    );

    println!(
        "{} {}",
        "Gemini is querying alpha vantage for:"
            .color(Color::Cyan)
            .bold(),
        symbol
    );

    let response = client
        .get(&url)
        .send()
        .map_err(|e| format!("Alpha Vantage API request failed: {}", e))?;

    let response_text = response
        .text()
        .map_err(|e| format!("Failed to parse Alpha Vantage response: {}", e))?;

    Ok(response_text)
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
}

fn main() {
    dotenv::from_path(format!("{}/.gemini", env!("HOME"))).ok();
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in ~/.gemini");

    let chat_manager = Arc::new(Mutex::new(ChatManager::new(api_key)));
    let chat_manager_clone = Arc::clone(&chat_manager);

    ctrlc::set_handler(move || {
        let mut manager = chat_manager_clone.lock().unwrap();
        manager.cleanup(true);
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

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

        print!(
            "{}",
            format!("[{}] > ", conv_length).color(Color::Green).bold()
        );
        io::stdout().flush().unwrap();

        let mut user_input = String::new();
        io::stdin().read_line(&mut user_input).unwrap();
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
            let output = execute_command(command, true);
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
            let mut response = match chat_manager.lock().unwrap().send_message(user_input) {
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

            loop {
                let tool_calls: Vec<(String, Value)> = response
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
                                let result = execute_command(cmd, false);
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
                                //println!("Search result: {}", result); // Log the raw result
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
                                    println!("Scrape failed: {}", result); // Log errors explicitly
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
                                let result = send_email(subj, bod);
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
                        _ => {
                            results.push(format!("[Tool error] Unknown function: {}", func_name));
                        }
                    }
                }

                if !results.is_empty() {
                    let combined_results = results.join("\n");
                    //println!("Sending to LLM: {}", combined_results); // Log what’s being sent
                    response = match chat_manager.lock().unwrap().send_message(&combined_results) {
                        Ok(resp) => resp,
                        Err(e) => {
                            println!("{}", format!("Error: {}", e).color(Color::Red));
                            break;
                        }
                    };
                    display_response(&response);
                }
            }
        }
    }

    chat_manager.lock().unwrap().cleanup(false);
}
