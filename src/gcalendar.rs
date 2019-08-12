use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use chrono::prelude::*;
// oauth2 v3 crate api is awful but v1 doesn't handle errors from the server properly
use oauth2::basic::{BasicClient, BasicTokenType};
use oauth2::reqwest::http_client;
use oauth2::TokenResponse;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EmptyExtraTokenFields,
    RedirectUrl, RefreshToken, ResponseType, Scope, StandardTokenResponse, TokenUrl,
};
use serde::Deserialize;
use time::Duration;
use url::Url;

use crate::config::{Config, GoogleToken};
use crate::report::*;

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

// Work with Google Calendar API

pub struct Calendar<'a> {
    client: oauth2::basic::BasicClient,
    config: &'a Config,
}

impl Calendar<'_> {
    pub fn new(cfg: &Config) -> Calendar {
        let auth_url =
            AuthUrl::new(Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap());
        let token_url =
            TokenUrl::new(Url::parse("https://www.googleapis.com/oauth2/v4/token").unwrap());

        let client_cfg = cfg.google_client.as_ref().unwrap();
        let client = BasicClient::new(
            ClientId::new(String::from(&client_cfg.client_id)),
            Some(ClientSecret::new(String::from(&client_cfg.client_secret))),
            auth_url,
            Some(token_url),
        )
        .set_redirect_url(RedirectUrl::new(
            Url::parse("http://localhost:7890").unwrap(),
        ));

        Calendar {
            client,
            config: cfg,
        }
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

    // the server would panic if anything goes wrong, not sure if I really need to fix it
    pub fn listen_for_code(&self) -> GoogleToken {
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
                        .expect("can't get access token");

                    return Self::config_from_token(token);
                }
                // ignore non-ok connections
                _ => continue,
            }
        }

        panic!("server stopped listening for connections");
    }

    // FIXME figure out how to use trait here instead of the type
    fn config_from_token(
        token: &StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType>,
    ) -> GoogleToken {
        let access_token = String::from(token.access_token().secret());
        let refresh_token = String::from(
            token
                .refresh_token()
                .expect("token must have refresh_token")
                .secret(),
        );
        let experies_at = Utc::now()
            + Duration::from_std(token.expires_in().expect("token must have expires_in")).unwrap();

        GoogleToken {
            access_token,
            refresh_token,
            experies_at,
        }
    }

    fn access_token(&self) -> Result<String, String> {
        match &self.config.google_token {
            Some(s) => Ok(s.access_token.clone()),
            None => Err(String::from("no token config")),
        }
    }

    pub fn refresh_if_needed(&self) -> Result<Option<GoogleToken>, String> {
        let experies_at = match &self.config.google_token {
            Some(s) => s.experies_at,
            None => return Err(String::from("no token config")),
        };

        // FIXME need some buffer here
        if experies_at < Utc::now() {
            Ok(Some(self.refresh_token()?))
        } else {
            Ok(None)
        }
    }

    fn refresh_token(&self) -> Result<GoogleToken, String> {
        let saved_token = match &self.config.google_token {
            Some(s) => s,
            None => return Err(String::from("no token in config")),
        };

        let token = self
            .client
            .exchange_refresh_token(&RefreshToken::new(saved_token.refresh_token.clone()))
            .request(http_client)
            .map_err(|e| format!("Can't refresh token: {}", e))?;

        let access_token = String::from(token.access_token().secret());
        let experies_at = Utc::now()
            + Duration::from_std(token.expires_in().expect("token must have expires_in")).unwrap();

        let refresh_token = match token.refresh_token() {
            Some(rt) => String::from(rt.secret()),
            None => saved_token.refresh_token.clone(),
        };

        Ok(GoogleToken {
            access_token,
            refresh_token,
            experies_at,
        })
    }

    pub fn list(&self) -> Result<Vec<ListItem>, String> {
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
        &self,
        since: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
    ) -> Result<Vec<Entry>, String> {
        let mut resp = reqwest::Client::new()
            .get(&format!(
                "https://www.googleapis.com/calendar/v3/calendars/{}/events?singleEvents=true&timeMin={}&timeMax={}&access_token={}",
                self.config.gcal.as_ref().unwrap().id,
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
