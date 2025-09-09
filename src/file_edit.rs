use once_cell::sync::Lazy;
use regex::Regex;
use std::fs;
use std::path::PathBuf;

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

pub fn file_editor(
    subcommand: &str,
    filename: &str,
    data: Option<&str>,
    replacement: Option<&str>,
) -> String {
    let file_path = PathBuf::from(&*SANDBOX_ROOT).join(filename);

    match subcommand {
        "read" => match fs::read_to_string(&file_path) {
            Ok(content) => format!("File contents:\n{}", content),
            Err(e) => format!("Error reading file '{}': {}", filename, e),
        },
        "write" => {
            let content = data.unwrap_or("");
            match fs::write(&file_path, content) {
                Ok(()) => format!("Successfully wrote to '{}'", filename),
                Err(e) => format!("Error writing to '{}': {}", filename, e),
            }
        }
        "search" => {
            let pattern = match data {
                Some(p) => p,
                None => {
                    return "Error: 'data' parameter with regex pattern is required for search"
                        .to_string()
                }
            };
            match Regex::new(pattern) {
                Ok(re) => match fs::read_to_string(&file_path) {
                    Ok(content) => {
                        let matches: Vec<_> = re.find_iter(&content).collect();
                        if matches.is_empty() {
                            format!(
                                "No matches found for pattern '{}' in '{}'",
                                pattern, filename
                            )
                        } else {
                            let match_list: Vec<String> = matches
                                .iter()
                                .map(|m| format!(" - {} (at position {})", m.as_str(), m.start()))
                                .collect();
                            format!(
                                "Found {} matches for pattern '{}' in '{}':\n{}",
                                matches.len(),
                                pattern,
                                filename,
                                match_list.join("\n")
                            )
                        }
                    }
                    Err(e) => format!("Error reading file '{}': {}", filename, e),
                },
                Err(e) => format!("Error compiling regex pattern '{}': {}", pattern, e),
            }
        }
        "search_and_replace" => {
            let pattern = match data {
                Some(p) => p,
                None => return "Error: 'data' parameter with regex pattern is required for search_and_replace".to_string(),
            };
            let replace_with = match replacement {
                Some(r) => r,
                None => {
                    return "Error: 'replacement' parameter is required for search_and_replace"
                        .to_string()
                }
            };
            match Regex::new(pattern) {
                Ok(re) => match fs::read_to_string(&file_path) {
                    Ok(content) => {
                        let new_content = re.replace_all(&content, replace_with);
                        match fs::write(&file_path, new_content.as_ref()) {
                            Ok(()) => format!(
                                "Successfully replaced pattern '{}' with '{}' in '{}'",
                                pattern, replace_with, filename
                            ),
                            Err(e) => format!("Error writing to '{}': {}", filename, e),
                        }
                    }
                    Err(e) => format!("Error reading file '{}': {}", filename, e),
                },
                Err(e) => format!("Error compiling regex pattern '{}': {}", pattern, e),
            }
        }
        "apply_diff" => {
            let diff_content = match data {
                Some(d) => d,
                None => {
                    return "Error: 'data' parameter with diff content is required for apply_diff"
                        .to_string()
                }
            };
            
            match fs::read_to_string(&file_path) {
                Ok(original_content) => {
                    // Parse and apply the diff
                    match apply_patch(&original_content, diff_content) {
                        Ok(new_content) => {
                            // Write the new content back to the file
                            match fs::write(&file_path, &new_content) {
                                Ok(()) => format!("Successfully applied diff to '{}'", filename),
                                Err(e) => format!("Error writing to '{}': {}", filename, e),
                            }
                        },
                        Err(e) => format!("Error parsing or applying diff: {}", e),
                    }
                }
                Err(e) => format!("Error reading file '{}': {}", filename, e),
            }
        }
        _ => format!("Error: Unknown subcommand '{}'", subcommand),
    }
}

fn apply_patch(original: &str, diff: &str) -> Result<String, String> {
    let original_lines: Vec<&str> = original.lines().collect();
    let mut result_lines = original_lines.clone();
    
    // Current position in the parsing of the diff
    let mut current_section_start_line = 0;
    let mut in_hunk = false;
    
    // Regular expression for unified diff hunk headers: @@ -a,b +c,d @@
    let hunk_header_re = Regex::new(r"@@ -(\d+),(\d+) \+(\d+),(\d+) @@").map_err(|e| e.to_string())?;
    
    // Process the diff line by line
    for line in diff.lines() {
        // Check if this is a hunk header line
        if let Some(caps) = hunk_header_re.captures(line) {
            in_hunk = true;
            
            // Parse the line numbers and counts from the hunk header
            let original_start: usize = caps[1].parse().map_err(|_| "Invalid line number in diff".to_string())?;
            let _original_count: usize = caps[2].parse().map_err(|_| "Invalid line count in diff".to_string())?;
            
            // In unified diffs, line numbers are 1-based, so we subtract 1 for 0-based indexing
            current_section_start_line = original_start - 1;
            continue;
        }
        
        // Skip file header lines in unified diff
        if line.starts_with("---") || line.starts_with("+++") {
            continue;
        }
        
        // If we're in a hunk, process addition/removal/context lines
        if in_hunk {
            match line.chars().next() {
                Some('+') => {
                    // Addition line: insert at current position
                    let content = &line[1..]; // Skip the '+' prefix
                    result_lines.insert(current_section_start_line, content);
                    current_section_start_line += 1;
                },
                Some('-') => {
                    // Removal line: remove at current position
                    if current_section_start_line < result_lines.len() {
                        result_lines.remove(current_section_start_line);
                    } else {
                        return Err(format!("Diff removal line {} is out of bounds", current_section_start_line));
                    }
                },
                Some(' ') => {
                    // Context line: just advance position
                    current_section_start_line += 1;
                },
                _ => {
                    // Other lines in the diff (could be comments, etc.)
                    // Ignore them
                }
            }
        }
    }
    
    Ok(result_lines.join("\n"))
}