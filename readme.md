# Gemini CLI

## Description

Gemini CLI is a Rust application that acts as a proactive assistant within a sandboxed multi-platform terminal environment. It leverages the Gemini 2.5 Flash API to assist with coding tasks, file operations, online searches, email sending, and shell commands. The application takes initiative to provide solutions, execute commands, and analyze results without explicit user confirmation, unless the action is ambiguous or potentially destructive.

## Functionality

*   **Chat Interface:** Provides a command-line interface for interacting with the Gemini AI model.
*   **Tool Execution:** Executes system commands using the `execute_command` function, allowing the AI to interact with the file system and other system utilities.
*   **Online Search:** Performs online searches using the `search_online` function, enabling the AI to retrieve up-to-date information from the web.
*   **Email Sending:** Sends emails using the `send_email` function, allowing the AI to send notifications or reports.
*   **Conversation History:** Maintains a conversation history to provide context for the AI model.
*   **Ctrl+C Handling:** Gracefully shuts down the application and cleans up resources when Ctrl+C is pressed.

## Modules

*   `src/main.rs`: Contains the main application logic, including the chat interface, tool execution, and API interaction.
*   `src/search.rs`: Implements the online search functionality using the Google Custom Search API and web scraping capabilities.
*   `src/command.rs`: Handles system command execution with sandboxing and security considerations.
*   `src/email.rs`: Manages email sending functionality with SMTP support.
*   `src/alpha_vantage.rs`: Provides integration with the Alpha Vantage API for financial data.
*   `src/file_edit.rs`: Implements file editing capabilities including reading, writing, searching, and applying diffs.
*   `src/spinner.rs`: Provides a loading spinner for visual feedback during operations.

## Configuration Setup

To run Gemini CLI, you need to set up a `.gemini` file in your home directory with the following variables:

```
GEMINI_API_KEY=<YOUR_GEMINI_API_KEY>
GOOGLE_SEARCH_API_KEY=<YOUR_GOOGLE_SEARCH_API_KEY>
GOOGLE_SEARCH_ENGINE_ID=<YOUR_GOOGLE_SEARCH_ENGINE_ID>
DESTINATION_EMAIL=<YOUR_DESTINATION_EMAIL>
SMTP_SERVER_IP=localhost
SENDER_EMAIL=<YOUR_SENDER_EMAIL>  # Optional, defaults to DESTINATION_EMAIL
SMTP_USERNAME=<YOUR_SMTP_USERNAME>  # Optional, required for non-localhost servers
SMTP_PASSWORD=<YOUR_SMTP_PASSWORD>  # Optional, required for non-localhost servers
```

*   `GEMINI_API_KEY`: Your API key for the Gemini 2.0 Flash API.
*   `GOOGLE_SEARCH_API_KEY`: Your API key for the Google Custom Search API.
*   `GOOGLE_SEARCH_ENGINE_ID`: Your search engine ID for the Google Custom Search API.
*   `DESTINATION_EMAIL`: The email address to which the `send_email` function will send emails.
*   `SMTP_SERVER_IP`: The IP address or hostname of the SMTP server (defaults to localhost if not specified).
*   `SENDER_EMAIL`: The email address to use as the sender (optional, defaults to DESTINATION_EMAIL).
*   `SMTP_USERNAME`: Username for SMTP authentication (optional, required for non-localhost servers).
*   `SMTP_PASSWORD`: Password for SMTP authentication (optional, required for non-localhost servers).

**Note:** Ensure that you have the necessary API keys and permissions to use the Gemini 2.0 Flash API and the Google Custom Search API.

## Usage

1.  Clone the repository:

    ```bash
    git clone <repository_url>
    cd gemini
    ```

2.  Create a `.gemini` file in your home directory and set the required environment variables as described in the Configuration Setup section.

3.  Run the application:

    ```bash
    cargo run
    ```

4.  Chat with Gemini by typing messages in the command-line interface. Use `!command` to run shell commands directly (e.g., `!ls` or `!dir`). Type `exit` to quit or `clear` to reset the conversation.