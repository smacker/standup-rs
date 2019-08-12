use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Github {
    pub username: String,
    pub token: String,
}

#[derive(Serialize, Deserialize)]
pub struct GoogleClient {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Serialize, Deserialize)]
pub struct GoogleToken {
    pub access_token: String,
    pub refresh_token: String,
    pub experies_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct GoogleCalendar {
    pub id: String,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub github: Github,
    pub google_client: Option<GoogleClient>,
    pub google_token: Option<GoogleToken>,
    pub gcal: Option<GoogleCalendar>,
}

impl Config {
    pub fn load(file_path: &Path) -> Result<Option<Config>, String> {
        if !file_path.exists() {
            return Ok(None);
        }

        let mut file = File::open(&file_path).map_err(|e| format!("can not open file: {}", e))?;
        let mut json = String::new();
        file.read_to_string(&mut json)
            .map_err(|e| format!("can not read file: {}", e))?;

        let cfg: Config =
            serde_json::from_str(&json).map_err(|e| format!("can not deserialize file: {}", e))?;

        Ok(Some(cfg))
    }

    pub fn save(&self, file_path: &PathBuf) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self)
            .map_err(|e| format!("can not serialize config file: {}", e))?;

        let path = Path::new(&file_path);
        let mut file =
            File::create(&path).map_err(|e| format!("can not open config file: {}", e))?;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("can not write config file: {}", e))?;

        Ok(())
    }
}
