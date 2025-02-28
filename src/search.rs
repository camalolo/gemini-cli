use std::collections::{HashMap, HashSet};
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::env;
use urlencoding;
use colored::{Color, Colorize};

pub const RELEVANCE_THRESHOLD: f32 = 0.05;

pub fn search_online(query: &str) -> String {
    let api_key = env::var("GOOGLE_SEARCH_API_KEY").expect("GOOGLE_SEARCH_API_KEY not set");
    let cx = env::var("GOOGLE_SEARCH_ENGINE_ID").expect("GOOGLE_SEARCH_ENGINE_ID not set");

    println!(
        "{} {}",
        "Gemini is searching online for:".color(Color::Cyan).bold(),
        query
    );
    
    let client = Client::new();
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
                let mut search_results = Vec::new();

                for item in items {
                    let title = item
                        .get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or("No title");
                    let link = item
                        .get("link")
                        .and_then(|l| l.as_str())
                        .unwrap_or("No link");

                    println!(
                        "{} {}",
                        "Gemini is reading:".color(Color::Cyan).bold(),
                        link
                    );

                    let content = match client.get(link).send() {
                        Ok(resp) if resp.status().is_success() => {
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
                        }
                        Ok(resp) => format!("Skipped due to HTTP status: {}", resp.status()),
                        Err(e) => format!("Error fetching {}: {}", link, e),
                    };

                    search_results.push((title.to_string(), link.to_string(), content));
                }

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
                    "No relevant results found, please try a different search query.".to_string()
                } else {
                    serde_json::to_string(&filtered_results)
                        .unwrap_or("Error serializing results".to_string())
                }
            } else {
                "No results found.".to_string()
            }
        }
        Err(e) => format!("Search failed: {}", e),
    }
}

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