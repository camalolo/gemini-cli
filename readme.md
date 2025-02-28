# Gemini

## Description

Gemini is a Rust application that acts as a proactive assistant within a sandboxed Linux terminal environment. It leverages the Gemini 2.0 Flash API to assist with coding tasks, file operations, online searches, email sending, and shell commands. The application takes initiative to provide solutions, execute commands, and analyze results without explicit user confirmation, unless the action is ambiguous or potentially destructive.

## Functionality

*   **Chat Interface:** Provides a command-line interface for interacting with the Gemini AI model.
*   **Tool Execution:** Executes system commands using the `execute_command` function, allowing the AI to interact with the file system and other system utilities.
*   **Online Search:** Performs online searches using the `search_online` function, enabling the AI to retrieve up-to-date information from the web.
*   **Email Sending:** Sends emails using the `send_email` function, allowing the AI to send notifications or reports.
*   **Conversation History:** Maintains a conversation history to provide context for the AI model.
*   **Ctrl+C Handling:** Gracefully shuts down the application and cleans up resources when Ctrl+C is pressed.

## Modules

*   `src/main.rs`: Contains the main application logic, including the chat interface, tool execution, and API interaction.
*   `src/search.rs`: Implements the online search functionality using the Google Custom Search API.

## .env Setup

To run Gemini, you need to set up a `.env` file in the project root directory with the following variables:

```
GEMINI_API_KEY=<YOUR_GEMINI_API_KEY>
GOOGLE_SEARCH_API_KEY=<YOUR_GOOGLE_SEARCH_API_KEY>
GOOGLE_SEARCH_ENGINE_ID=<YOUR_GOOGLE_SEARCH_ENGINE_ID>
DESTINATION_EMAIL=<YOUR_DESTINATION_EMAIL>
```

*   `GEMINI_API_KEY`: Your API key for the Gemini 2.0 Flash API.
*   `GOOGLE_SEARCH_API_KEY`: Your API key for the Google Custom Search API.
*   `GOOGLE_SEARCH_ENGINE_ID`: Your search engine ID for the Google Custom Search API.
*   `DESTINATION_EMAIL`: The email address to which the `send_email` function will send emails.

**Note:** Ensure that you have the necessary API keys and permissions to use the Gemini 2.0 Flash API and the Google Custom Search API.

## Usage

1.  Clone the repository:

    ```bash
    git clone <repository_url>
    cd gemini
    ```

2.  Create a `.env` file in the project root directory and set the required environment variables as described in the `.env Setup` section.

3.  Run the application:

    ```bash
    cargo run
    ```

4.  Chat with Gemini by typing messages in the command-line interface. Use `!command` to run shell commands directly (e.g., `!ls` or `!dir`). Type `exit` to quit or `clear` to reset the conversation.