use chrono::Local;
use colored::{Color, Colorize};
use ctrlc;
use dotenv::dotenv;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::env;
use std::io::{self, Write};
use std::process::Command;
use std::sync::{Arc, Mutex};

// Declare and import the search module
mod search;
use search::search_online;

const SANDBOX_ROOT: &str = ".";

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
            "Today's date is {}. You are a proactive coding assistant running in a sandboxed Linux terminal environment with a full set of command line utilities. Your role is to assist with coding tasks, file operations, and shell commands efficiently and decisively. Assume the current directory (the sandbox root) is the target for all commands. Take initiative to provide solutions, execute commands, and analyze results immediately without asking for confirmation unless the action is explicitly ambiguous (e.g., multiple repos) or potentially destructive (e.g., deleting files). Use the `execute_command` tool to interact with the system when needed, and deliver concise, clear responses. After running a command, always summarize its output immediately and proceed with logical next steps, without waiting for the user to prompt you further. When reading files or executing commands, summarize the results intelligently for the user without dumping raw output unless explicitly requested. Stay within the sandbox directory. Users can run shell commands directly with `!`, and you'll receive the output to assist further. Act confidently and anticipate the user's needs to streamline their workflow.",
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
            std::thread::sleep(std::time::Duration::from_secs(if is_signal { 3 } else { 2 }));
        }
    }
}

fn execute_command(command: &str, skip_confirm: bool) -> String {
    if !skip_confirm {
        println!(
            "{}Gemini wants to run the following command: {}",
            "cyan".color(Color::Cyan).bold(),
            command
        );
        print!(
            "{}Press Enter to confirm, anything else to cancel: ",
            "green".color(Color::Green)
        );
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();
        if input.is_empty() {
            println!("{}", "Command confirmed.".color(Color::Green));
        } else {
            return format!(
                "Command canceled by user who asked the following: {}",
                input
            );
        }
    }

    println!(
        "{}",
        format!("Executing command: {}", command).color(Color::Yellow)
    );

    let (shell, arg) = if cfg!(target_os = "windows") {
        ("cmd", "/c")
    } else {
        ("sh", "-c")
    };

    let output = Command::new(shell)
        .arg(arg)
        .arg(command)
        .current_dir(SANDBOX_ROOT)
        .output()
        .map(|out| {
            if out.status.success() {
                String::from_utf8_lossy(&out.stdout).to_string()
            } else {
                String::from_utf8_lossy(&out.stderr).to_string()
            }
        })
        .unwrap_or_else(|e| {
            format!("Error executing '{}': {}", command, e)
                .color(Color::Red)
                .to_string()
        });

    println!(
        "{}",
        format!("Command output: {}", output).color(Color::Yellow)
    );
    output
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
                        println!("{}", text.color(Color::Blue));
                    }
                }
            }
        }
    }
}

fn main() {
    dotenv().ok();
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in .env file");
    let _ = env::var("GOOGLE_SEARCH_API_KEY").expect("GOOGLE_SEARCH_API_KEY not set");
    let _ = env::var("GOOGLE_SEARCH_ENGINE_ID").expect("GOOGLE_SEARCH_ENGINE_ID not set");

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
        format!("Working in sandbox: {}", SANDBOX_ROOT).color(Color::Cyan)
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
                                .filter_map(|part| part.get("text").and_then(|t| t.as_str()).map(|s| s.len()))
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
                println!(
                    "{}",
                    "Please enter a command or message.".color(Color::Yellow)
                );
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
                        _ => {
                            results.push(format!("[Tool error] Unknown function: {}", func_name));
                        }
                    }
                }

                if !results.is_empty() {
                    let combined_results = results.join("\n");
                    //println!("Sending to LLM: {}", combined_results);
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
