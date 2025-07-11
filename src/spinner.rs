use std::io::{self, Write};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;

pub struct Spinner {
    handle: Option<thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl Spinner {
    pub fn new() -> Self {
        Spinner {
            handle: None,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&mut self) { // Removed message parameter
        if self.running.load(Ordering::SeqCst) {
            return; // Already running
        }

        self.running.store(true, Ordering::SeqCst);
        let running_flag = Arc::clone(&self.running);

        self.handle = Some(thread::spawn(move || {
            let chars = ['-', '\\', '|', '/'];
            let mut i = 0;
            while running_flag.load(Ordering::SeqCst) {
                print!("\r{}", chars[i]); // Only print the character
                io::stdout().flush().unwrap();
                i = (i + 1) % chars.len();
                thread::sleep(Duration::from_millis(100)); // Adjust speed here
            }
            // Clear the spinner line after stopping
            print!("\r{}", " ".repeat(80)); // Clear the entire line with spaces
            print!("\r"); // Move cursor back to the beginning of the line
            io::stdout().flush().unwrap();
        }));
    }

    pub fn stop(&mut self) {
        if self.running.load(Ordering::SeqCst) {
            self.running.store(false, Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                handle.join().unwrap(); // Wait for the spinner thread to finish
            }
        }
    }
}

// Ensure the spinner stops even if the main thread panics or exits
impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}