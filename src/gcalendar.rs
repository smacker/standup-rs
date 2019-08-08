use chrono::prelude::*;
use dirs::home_dir;
// oauth2 v3 crate api is awful but v1 doesn't handle errors from the server properly
use oauth2::basic::{BasicClient, BasicTokenType};
use oauth2::reqwest::http_client;
use oauth2::TokenResponse;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EmptyExtraTokenFields,
    RedirectUrl, RefreshToken, ResponseType, Scope, StandardTokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::prelude::*;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use time::Duration;
use url::Url;

#[path = "report.rs"]
mod report;
use report::*;

// Google calendar structs

#[derive(Deserialize)]
struct ListResp {
    items: Vec<ListItem>,
}

#[derive(Deserialize)]
pub struct ListItem {
    pub id: String,
    pub summary: String,
}

#[derive(Deserialize)]
struct EventsResp {
    items: Vec<Event>,
}

#[derive(Deserialize)]
struct Event {
    status: String,
    summary: String,
}

// refresh token must be saved somewhere to obtain new access_tokens

#[derive(Serialize, Deserialize)]
struct TokenStorage {
    access_token: String,
    refresh_token: String,
    experies_at: DateTime<Utc>,
}

impl TokenStorage {
    fn load(file_path: &Path) -> Result<Option<TokenStorage>, String> {
        if !file_path.exists() {
            return Ok(None);
        }

        let mut file = File::open(&file_path).map_err(|e| format!("can not open file: {}", e))?;
        let mut json = String::new();
        file.read_to_string(&mut json)
            .map_err(|e| format!("can not read file: {}", e))?;

        let storage: TokenStorage =
            serde_json::from_str(&json).map_err(|e| format!("can not deserialize file: {}", e))?;

        Ok(Some(storage))
    }

    // FIXME figure out how to use trait here instead of the type
    // FIXME don't panic
    fn from_token(
        token: &StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType>,
    ) -> Result<TokenStorage, String> {
        let access_token = String::from(token.access_token().secret());
        let refresh_token = String::from(
            token
                .refresh_token()
                .expect("token must have refresh_token")
                .secret(),
        );
        let experies_at = Utc::now()
            + Duration::from_std(token.expires_in().expect("token must have expires_in")).unwrap();

        Ok(TokenStorage {
            access_token,
            refresh_token,
            experies_at,
        })
    }

    // FIXME figure out how to use trait here instead of the type
    // FIXME don't panic
    fn update(
        &mut self,
        token: &StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType>,
    ) -> Result<(), String> {
        self.access_token = String::from(token.access_token().secret());
        self.experies_at = Utc::now()
            + Duration::from_std(token.expires_in().expect("token must have expires_in")).unwrap();

        if let Some(rt) = token.refresh_token() {
            self.refresh_token = String::from(rt.secret());
        }

        Ok(())
    }

    fn save(&self, file_path: &PathBuf) -> Result<(), String> {
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

// Work with Google Calendar API

pub struct Calendar {
    client: oauth2::basic::BasicClient,
    storage: Option<TokenStorage>,
    storage_file: PathBuf,
}

impl Calendar {
    // FIXME make this function return error
    pub fn new(client_id: &str, secret_id: &str) -> Calendar {
        let auth_url =
            AuthUrl::new(Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap());
        let token_url =
            TokenUrl::new(Url::parse("https://www.googleapis.com/oauth2/v4/token").unwrap());

        let client = BasicClient::new(
            ClientId::new(String::from(client_id)),
            Some(ClientSecret::new(String::from(secret_id))),
            auth_url,
            Some(token_url),
        )
        .set_redirect_url(RedirectUrl::new(
            Url::parse("http://localhost:7890").unwrap(),
        ));

        let storage_file = Path::join(&home_dir().unwrap(), ".standup");
        let storage = TokenStorage::load(&storage_file).unwrap();
        Calendar {
            client,
            storage,
            storage_file,
        }
    }

    pub fn authorized(&self) -> bool {
        self.storage.is_some()
    }

    pub fn authorize_url(&self) -> String {
        let (url, _) = self
            .client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/calendar.readonly".to_string(),
            ))
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/calendar.events.readonly".to_string(),
            ))
            .set_response_type(&ResponseType::new("code".to_string()))
            .url();
        String::from(url.as_str())
    }

    // the server would panic in case anything goes wrong
    pub fn listen_for_code(&mut self) -> Result<(), String> {
        let listener = TcpListener::bind("127.0.0.1:7890").expect("can not open 7890 port");
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let mut reader = BufReader::new(&stream);
                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();

                    let redirect_url = request_line.split_whitespace().nth(1).unwrap();
                    let url = Url::parse(&("http://localhost".to_string() + redirect_url)).unwrap();

                    let code_pair = url
                        .query_pairs()
                        .find(|pair| {
                            let &(ref key, _) = pair;
                            key == "code"
                        })
                        .unwrap();

                    let (_, value) = code_pair;
                    let code = AuthorizationCode::new(value.into_owned());

                    let message = "Go back to your terminal :)";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                        message.len(),
                        message
                    );
                    stream.write_all(response.as_bytes()).unwrap();

                    let token = &self
                        .client
                        .exchange_code(code)
                        .request(http_client)
                        .map_err(|e| format!("Can't get access token: {}", e))?;

                    let storage = TokenStorage::from_token(token)?;
                    self.storage = Some(storage);

                    // don't forget to stop the server
                    break;
                }
                // ignore non-ok connections
                _ => continue,
            }
        }

        Ok(())
    }

    fn access_token(&self) -> Result<String, String> {
        match &self.storage {
            Some(s) => Ok(s.access_token.clone()),
            None => Err(String::from("no storage")),
        }
    }

    fn refresh_if_needed(&mut self) -> Result<(), String> {
        let experies_at = match &self.storage {
            Some(s) => s.experies_at,
            None => return Err(String::from("no storage")),
        };

        // FIXME need some buffer here
        if experies_at < Utc::now() {
            self.refresh_token()?;
        }
        Ok(())
    }

    fn refresh_token(&mut self) -> Result<(), String> {
        let storage = match &mut self.storage {
            Some(s) => s,
            None => return Err(String::from("no storage")),
        };

        let token = self
            .client
            .exchange_refresh_token(&RefreshToken::new(storage.refresh_token.clone()))
            .request(http_client)
            .map_err(|e| format!("Can't refresh token: {}", e))?;

        storage.update(&token)?;
        storage.save(&self.storage_file)
    }

    pub fn list(&mut self) -> Result<Vec<ListItem>, String> {
        self.refresh_if_needed()?;

        let mut resp = reqwest::Client::new()
            .get(&format!(
                "https://www.googleapis.com/calendar/v3/users/me/calendarList?access_token={}",
                self.access_token()?,
            ))
            .send()
            .map_err(|e| format!("Request to Google Calendar failed: {}", e))?
            .error_for_status()
            .map_err(|e| format!("Incorrect response status: {}", e))?;

        let json: ListResp = resp
            .json()
            .map_err(|e| format!("Can not parse Google Calendar response: {}", e))?;

        Ok(json.items)
    }

    pub fn events(
        &mut self,
        since: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
        calendar_id: String,
    ) -> Result<Vec<Entry>, String> {
        self.refresh_if_needed()?;

        let mut resp = reqwest::Client::new()
            .get(&format!(
                "https://www.googleapis.com/calendar/v3/calendars/{}/events?singleEvents=true&timeMin={}&timeMax={}&access_token={}",
                calendar_id,
                since.to_rfc3339_opts(SecondsFormat::Secs, true),
                until.unwrap_or_else(Utc::now).to_rfc3339_opts(SecondsFormat::Secs, true),
                self.access_token()?,
            ))
            .send()
            .map_err(|e| format!("Request to Google Calendar failed: {}", e))?
            .error_for_status()
            .map_err(|e| format!("Incorrect response status: {}", e))?;

        let json: EventsResp = resp
            .json()
            .map_err(|e| format!("Can not parse Google Calendar response: {}", e))?;

        let events: Vec<_> = json
            .items
            .iter()
            .filter(|x| x.status == "confirmed")
            .map(|x| Entry {
                r#type: String::from("Meeting"),
                title: x.summary.clone(),
                url: None,
                actions: Vec::new(),
            })
            .collect();

        Ok(events)
    }
}
