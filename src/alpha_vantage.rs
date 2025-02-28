use colored::Color;
use colored::Colorize;
use reqwest::blocking::Client;
use std::env;

pub fn alpha_vantage_query(function: &str, symbol: &str) -> Result<String, String> {
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
