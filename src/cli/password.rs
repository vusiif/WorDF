//! Secure password credential resolution from various carrier options.
//! License: GPL-3.0-or-later

use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

#[derive(Default, Clone, Debug)]
pub struct PasswordOptions {
    pub raw: Option<String>,
    pub prompt: bool,
    pub file: Option<String>,
    pub stdin: bool,
    pub env_var: Option<String>,
}

impl PasswordOptions {
    pub fn validate_and_resolve(&self) -> Result<Option<String>, i32> {
        let count = self.raw.is_some() as usize
            + self.prompt as usize
            + self.file.is_some() as usize
            + self.stdin as usize
            + self.env_var.is_some() as usize;

        if count > 1 {
            eprintln!(
                "Error: Multiple password options specified. Please specify only one of --password, --password-prompt, --password-file, --password-stdin, or --password-env."
            );
            return Err(2);
        }

        if let Some(ref p) = self.raw {
            return Ok(Some(p.clone()));
        }

        if self.prompt {
            match rpassword::prompt_password("Enter decryption password: ") {
                Ok(p) => return Ok(Some(p)),
                Err(e) => {
                    eprintln!("Error: Failed to prompt for password: {}", e);
                    return Err(2);
                }
            }
        }

        if let Some(ref file_path) = self.file {
            match fs::read_to_string(Path::new(file_path)) {
                Ok(content) => {
                    let trimmed = content.trim_end_matches(&['\r', '\n'][..]).to_string();
                    return Ok(Some(trimmed));
                }
                Err(e) => {
                    eprintln!(
                        "Error: Failed to read password from file '{}': {}",
                        file_path, e
                    );
                    return Err(2);
                }
            }
        }

        if self.stdin {
            let stdin = io::stdin();
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) => {
                    eprintln!("Error: Failed to read password from stdin: unexpected EOF.");
                    return Err(2);
                }
                Ok(_) => {
                    let trimmed = line.trim_end_matches(&['\r', '\n'][..]).to_string();
                    return Ok(Some(trimmed));
                }
                Err(e) => {
                    eprintln!("Error: Failed to read password from stdin: {}", e);
                    return Err(2);
                }
            }
        }

        if let Some(ref env_name) = self.env_var {
            match std::env::var(env_name) {
                Ok(val) => return Ok(Some(val)),
                Err(_) => {
                    eprintln!(
                        "Error: Password environment variable '{}' is not set.",
                        env_name
                    );
                    return Err(2);
                }
            }
        }

        Ok(None)
    }
}
