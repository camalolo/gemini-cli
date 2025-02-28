use std::collections::{HashMap, HashSet};
use reqwest::blocking::{Client, ClientBuilder};
use reqwest::StatusCode;
use std::time::Duration;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::env;
use urlencoding;
use colored::{Color, Colorize};
use std::sync::{Arc, Mutex};
use std::thread;

pub const RELEVANCE_THRESHOLD: f32 = 0.05;
pub const NETWORK_TIMEOUT: u64 = 30;

pub fn search_online(query: &str) -> String {
    let api_key = env::var("GOOGLE_SEARCH_API_KEY").expect("GOOGLE_SEARCH_API_KEY not found in ~/.gemini");
    let cx = env::var("GOOGLE_SEARCH_ENGINE_ID").expect("GOOGLE_SEARCH_ENGINE_ID not found in ~/.gemini");

    println!(
        "{} {}",
        "Gemini is searching online for:".color(Color::Cyan).bold(),
        query
    );
    
    // Create a client with timeout
    let client = ClientBuilder::new()
        .connect_timeout(Duration::from_secs(NETWORK_TIMEOUT))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .build()
        .unwrap_or_else(|_| Client::new());
        
    let url = format!(
        "https://www.googleapis.com/customsearch/v1?key={}&cx={}&q={}",
        api_key,
        cx,
        urlencoding::encode(query)
    );

    match client.get(&url).send() {
        Ok(response) => {
            let json: Value = match response.json() {
                Ok(j) => j,
                Err(e) => return format!("Failed to parse search response: {}", e),
            };
            let items = json.get("items").and_then(|i| i.as_array());
            if let Some(items) = items {
                // Convert items to a Vec we can use for parallel processing
                let item_values: Vec<Value> = items.iter().cloned().collect();
                
                // Create thread-safe results container
                let search_results: Arc<Mutex<Vec<(String, String, String)>>> = 
                    Arc::new(Mutex::new(Vec::with_capacity(item_values.len())));
                
                // Create threads for parallel scraping
                let mut handles = vec![];
                
                for item in item_values {
                    // Clone shared resources for the thread
                    let client_clone = client.clone();
                    let search_results_clone = Arc::clone(&search_results);
                    
                    // Extract data before spawning the thread
                    let title = item
                        .get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or("No title")
                        .to_string();
                    let link = item
                        .get("link")
                        .and_then(|l| l.as_str())
                        .unwrap_or("No link")
                        .to_string();
                    
                    // Spawn a thread for each search result
                    let handle = thread::spawn(move || {
                        println!(
                            "{} {}",
                            "Gemini is reading:".color(Color::Cyan).bold(),
                            link
                        );

                        let content = match client_clone.get(&link).send() {
                            Ok(resp) => {
                                // Check status code first
                                match resp.status() {
                                    StatusCode::OK => {
                                        match resp.text() {
                                            Ok(text) => {
                                                let document = Html::parse_document(&text);
                                                // Target readable content: paragraphs, headings, articles
                                                let selector = Selector::parse("p, h1, h2, h3, h4, h5, h6, article").unwrap();
                                                let readable_text: Vec<String> = document
                                                    .select(&selector)
                                                    .flat_map(|element| {
                                                        // Only include text from elements not inside script/style
                                                        if element.value().name() != "script" && element.value().name() != "style" {
                                                            element.text().map(|t| t.trim().to_string()).collect::<Vec<_>>()
                                                        } else {
                                                            Vec::new()
                                                        }
                                                    })
                                                    .filter(|t| !t.is_empty()) // Skip empty strings
                                                    .collect();
                                                
                                                if readable_text.is_empty() {
                                                    "No readable content found on this page.".to_string()
                                                } else {
                                                    readable_text.join(" ")
                                                }
                                            }
                                            Err(e) => format!("Error reading content: {}", e),
                                        }
                                    },
                                    StatusCode::NOT_FOUND => "Skipped: 404 Not Found".to_string(),
                                    StatusCode::FORBIDDEN => "Skipped: 403 Forbidden".to_string(),
                                    StatusCode::INTERNAL_SERVER_ERROR => "Skipped: 500 Internal Server Error".to_string(),
                                    status => format!("Skipped: HTTP status {}", status),
                                }
                            },
                            Err(e) => {
                                if e.is_timeout() {
                                    format!("Skipped: Request timed out")
                                } else if e.is_connect() {
                                    format!("Skipped: Connection error")
                                } else {
                                    format!("Error fetching {}: {}", link, e)
                                }
                            }
                        };

                        // Store the result in our shared vector
                        search_results_clone.lock().unwrap().push((title, link, content));
                    });
                    
                    handles.push(handle);
                }
                
                // Wait for all threads to complete
                for handle in handles {
                    let _ = handle.join();
                }
                
                // Get the results from the Mutex
                let search_results = Arc::try_unwrap(search_results)
                    .expect("Arc still has multiple owners")
                    .into_inner()
                    .expect("Mutex is poisoned");

                let documents: Vec<&str> = search_results
                    .iter()
                    .filter_map(|(_, _, content)| {
                        if content.starts_with("Error") || content.starts_with("Skipped") {
                            None
                        } else {
                            Some(content.as_str())
                        }
                    })
                    .collect();

                if documents.is_empty() {
                    return "No valid content to process.".to_string();
                }

                let tfidf = compute_tfidf(&documents);
                let query_vector = tf_vector(query, &tfidf);
                let query_graph = build_term_graph(query);

                let mut scored_results: Vec<(f32, String, String, String)> = search_results
                    .into_iter()
                    .filter_map(|(title, link, content)| {
                        if content.starts_with("Error") || content.starts_with("Skipped") {
                            return None;
                        }

                        let doc_vector = tf_vector(&content, &tfidf);
                        let tfidf_similarity = cosine_similarity(&query_vector, &doc_vector);

                        let doc_graph = build_term_graph(&content);
                        let graph_similarity = graph_similarity(&query_graph, &doc_graph);

                        let combined_similarity = 0.7 * tfidf_similarity + 0.3 * graph_similarity;
                        //println!(
                        //    "Score for {}: TF-IDF={}, Graph={}, Combined={}",
                        //    link, tfidf_similarity, graph_similarity, combined_similarity
                        //);
                        Some((combined_similarity, title, link, content))
                    })
                    .collect();

                scored_results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                let filtered_results: Vec<_> = scored_results
                    .into_iter()
                    .filter(|(score, _, _, _)| *score >= RELEVANCE_THRESHOLD)
                    .map(|(_, title, link, content)| {
                        json!({
                            "title": title,
                            "link": link,
                            "content": content
                        })
                    })
                    .collect();

                if filtered_results.is_empty() {
                    "No relevant results found, please ask the user if your should try a different search query.".to_string()
                } else {
                    serde_json::to_string(&filtered_results)
                        .unwrap_or("Error serializing results".to_string())
                }
            } else {
                "No results found.".to_string()
            }
        }
        Err(e) => {
            if e.is_timeout() {
                "Search failed: Request timed out".to_string()
            } else {
                format!("Search failed: {}", e)
            }
        }
    }
}

// The rest of the functions remain unchanged
pub struct TfIdf {
    pub vocab: HashSet<String>,
    pub idf: HashMap<String, f32>,
}

fn compute_tfidf(documents: &[&str]) -> TfIdf {
    let mut vocab: HashSet<String> = HashSet::new();
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    let num_docs = documents.len() as f32;

    for doc in documents {
        let words: HashSet<String> = doc
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect();
        for word in &words {
            *doc_freq.entry(word.clone()).or_insert(0) += 1;
        }
        vocab.extend(words);
    }

    let mut idf: HashMap<String, f32> = HashMap::new();
    for word in &vocab {
        let df = *doc_freq.get(word).unwrap_or(&0) as f32;
        idf.insert(word.clone(), (num_docs / (df + 1.0)).ln() + 1.0);
    }

    TfIdf { vocab, idf }
}

fn tf_vector(text: &str, tfidf: &TfIdf) -> Vec<f32> {
    let mut word_counts: HashMap<String, usize> = HashMap::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let total_words = words.len() as f32;

    for word in words {
        *word_counts.entry(word.to_lowercase()).or_insert(0) += 1;
    }

    tfidf.vocab
        .iter()
        .map(|word| {
            let tf = *word_counts.get(word).unwrap_or(&0) as f32 / total_words;
            let idf = *tfidf.idf.get(word).unwrap_or(&1.0); // Default to 1.0 if not found (neutral weight)
            tf * idf // TF-IDF value
        })
        .collect()
}

fn cosine_similarity(vec1: &[f32], vec2: &[f32]) -> f32 {
    let dot_product: f32 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
    let norm1: f32 = vec1.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm2: f32 = vec2.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if norm1 == 0.0 || norm2 == 0.0 {
        0.0
    } else {
        dot_product / (norm1 * norm2)
    }
}

fn build_term_graph(content: &str) -> HashMap<String, HashSet<String>> {
    let mut graph: HashMap<String, HashSet<String>> = HashMap::new();
    let words: Vec<&str> = content.split_whitespace().collect();
    
    for i in 0..words.len().saturating_sub(1) {
        let w1 = words[i].to_lowercase();
        let w2 = words[i + 1].to_lowercase();
        graph.entry(w1.clone()).or_default().insert(w2.clone());
        graph.entry(w2).or_default().insert(w1);
    }
    
    graph
}

fn graph_similarity(query_graph: &HashMap<String, HashSet<String>>, doc_graph: &HashMap<String, HashSet<String>>) -> f32 {
    let query_terms: HashSet<_> = query_graph.keys().collect();
    let doc_terms: HashSet<_> = doc_graph.keys().collect();
    let intersection = query_terms.intersection(&doc_terms).count() as f32;
    let union = query_terms.union(&doc_terms).count() as f32;

    let term_similarity = if union == 0.0 { 0.0 } else { intersection / union };

    let mut edge_similarity_sum = 0.0;
    let mut shared_count = 0;
    let empty_set: HashSet<String> = HashSet::new();
    for term in query_terms.intersection(&doc_terms) {
        let query_edges = query_graph.get(*term).unwrap_or(&empty_set);
        let doc_edges = doc_graph.get(*term).unwrap_or(&empty_set);
        let edge_intersection = query_edges.intersection(doc_edges).count() as f32;
        let edge_union = query_edges.union(doc_edges).count() as f32;
        edge_similarity_sum += if edge_union == 0.0 { 0.0 } else { edge_intersection / edge_union };
        shared_count += 1;
    }
    
    let edge_similarity = if shared_count == 0 { 0.0 } else { edge_similarity_sum / shared_count as f32 };

    0.5 * term_similarity + 0.5 * edge_similarity
}