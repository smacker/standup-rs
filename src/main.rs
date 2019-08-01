// Supported events:
//
// PullRequestEvent - action: open/merged
// IssuesEvent - action: open
// PullRequestReviewEvent
// PullRequestReviewCommentEvent (merged with review event)
//
// TODO:
// IssueCommentEvent
// Another TODO: figure how to handle prs updates (push)

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io::{stderr, Write};
use std::process;

use chrono::prelude::*;
use reqwest::header::AUTHORIZATION;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use structopt::StructOpt;
use time::Duration;

// Github response structs

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Repo {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct User {
    login: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PullRequest {
    id: u64,
    html_url: String,
    title: String,
    #[serde(default)]
    merged: bool,
    user: User,
}

#[derive(Debug, Serialize, Deserialize)]
struct PullRequestEventPayload {
    action: String,
    number: u16,
    pull_request: PullRequest,
}

#[derive(Debug, Serialize, Deserialize)]
struct Issue {
    id: u64,
    html_url: String,
    title: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct IssuesEventPayload {
    action: String,
    issue: Issue,
}

#[derive(Debug, Serialize, Deserialize)]
struct PullRequestReviewPayload {
    action: String,
    pull_request: PullRequest,
}

#[derive(Debug, Serialize, Deserialize)]
struct PullRequestReviewCommentPayload {
    action: String,
    pull_request: PullRequest,
}

#[derive(Debug, Serialize, Deserialize)]
enum EventPayload {
    PullRequest(PullRequestEventPayload),
    Review(PullRequestReviewPayload),
    ReviewComment(PullRequestReviewCommentPayload),
    Issue(IssuesEventPayload),
}

#[derive(Debug, Serialize, Deserialize)]
struct Event {
    r#type: String,
    repo: Repo,
    #[serde(skip_deserializing)]
    payload: Option<EventPayload>,
    created_at: DateTime<Utc>,
}

// Result struct

#[derive(Clone)]
struct Entry {
    r#type: String,
    title: String,
    url: String,
    actions: Vec<String>,
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "[{}] ({}) {} {}",
            self.r#type,
            self.actions.join(", "),
            self.title,
            self.url
        )
    }
}

// Transformations

fn group_by_repos(events: Vec<&Event>) -> HashMap<&String, Vec<&Event>> {
    let mut res = HashMap::new();

    for e in events {
        let v = res.entry(&e.repo.name).or_insert(Vec::new());
        v.push(e);
    }

    res
}

fn convert(events: &Vec<&EventPayload>, login: &String) -> Vec<Entry> {
    let mut res = HashMap::new();

    for event in events {
        match event {
            EventPayload::PullRequest(p) => {
                let pr = &p.pull_request;
                let entry = res.entry(pr.id).or_insert(Entry {
                    r#type: String::from("PR"),
                    title: pr.title.clone(),
                    url: pr.html_url.clone(),
                    actions: Vec::new(),
                });

                let mut action = p.action.clone();
                if action == "closed" {
                    if !pr.merged {
                        continue;
                    }
                    action = String::from("merged");
                }
                if !entry.actions.contains(&action) {
                    entry.actions.push(action);
                }
            }
            EventPayload::Issue(p) => {
                if p.action != "created" {
                    continue;
                }

                let issue = &p.issue;
                let entry = res.entry(issue.id).or_insert(Entry {
                    r#type: String::from("Issue"),
                    title: issue.title.clone(),
                    url: issue.html_url.clone(),
                    actions: Vec::new(),
                });

                if !entry.actions.contains(&p.action) {
                    entry.actions.push(p.action.clone());
                }
            }
            EventPayload::Review(p) => {
                let pr = &p.pull_request;
                if &pr.user.login == login {
                    continue;
                }

                res.entry(pr.id).or_insert(Entry {
                    r#type: String::from("PR"),
                    title: pr.title.clone(),
                    url: pr.html_url.clone(),
                    actions: vec![String::from("reviewed")],
                });
            }
            EventPayload::ReviewComment(p) => {
                let pr = &p.pull_request;
                if &pr.user.login == login {
                    continue;
                }

                res.entry(pr.id).or_insert(Entry {
                    r#type: String::from("PR"),
                    title: pr.title.clone(),
                    url: pr.html_url.clone(),
                    actions: vec![String::from("reviewed")],
                });
            }
        }
    }

    res.values().map(|x| x.clone()).collect()
}

// FIXME there must be better way to do this
// I need something like adjacently tag on field, not a container
// untagged is also an option but it can occasionally match to a wrong struct
fn value_to_events(json: Value) -> serde_json::Result<Vec<Event>> {
    let mut es: Vec<Event> = serde_json::from_value(json.clone())?;
    for (i, v) in json.as_array().unwrap().iter().enumerate() {
        let payload = v.as_object().unwrap().get("payload").unwrap().clone();
        es[i].payload = match es[i].r#type.as_ref() {
            "PullRequestEvent" => {
                let p: PullRequestEventPayload = serde_json::from_value(payload)?;
                Some(EventPayload::PullRequest(p))
            }
            "IssuesEvent" => {
                let p: IssuesEventPayload = serde_json::from_value(payload)?;
                Some(EventPayload::Issue(p))
            }
            "PullRequestReviewEvent" => {
                let p: PullRequestReviewPayload = serde_json::from_value(payload)?;
                Some(EventPayload::Review(p))
            }
            "PullRequestReviewCommentEvent" => {
                let p: PullRequestReviewCommentPayload = serde_json::from_value(payload)?;
                Some(EventPayload::ReviewComment(p))
            }
            _ => None,
        }
    }
    Ok(es)
}

// Cli
#[derive(Debug, StructOpt)]
#[structopt(
    name = "standup-rs",
    about = "Generate a report for morning standup using Github."
)]
struct Opt {
    #[structopt(short = "u", long, env = "STANDUP_USER", empty_values = false)]
    /// Github user login
    user: String,

    #[structopt(short = "t", long, env = "STANDUP_GITHUB_TOKEN", empty_values = false)]
    /// Personal Github token
    token: String,

    #[structopt(
        short = "s",
        long,
        default_value = "yesterday",
        parse(try_from_str = "parse_since")
    )]
    /// Valid values: yesterday, friday, yyyy-mm-dd
    since: DateTime<Utc>,
}

fn parse_since(v: &str) -> Result<DateTime<Utc>, &str> {
    match v {
        "yesterday" => {
            let yesteday = Utc::today() - Duration::days(1);
            Ok(yesteday.and_hms(0, 0, 0))
        }
        "friday" => {
            let mut r = Utc::today();
            while r.weekday() != Weekday::Fri {
                r = r - Duration::days(1);
            }
            Ok(r.and_hms(0, 0, 0))
        }
        _ => {
            let r = NaiveDate::parse_from_str(v, "%Y-%m-%d");
            match r {
                Ok(v) => Ok(DateTime::from_utc(v.and_hms(0, 0, 0), Utc)),
                Err(_) => Err("unsupported value"),
            }
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let opt = Opt::from_args();

    // documentation says per_page isn't supported but it is :-D
    // TODO add pagination
    let json: Value = reqwest::Client::new()
        .get(&format!(
            "https://api.github.com/users/{}/events?per_page=100",
            opt.user
        ))
        .header(AUTHORIZATION, format!("token {}", opt.token))
        .send()
        .map_err(|e| format!("Request to Github failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Incorrect response status: {}", e))?
        .json()
        .map_err(|e| format!("Can not parse Github response: {}", e))?;

    let events = value_to_events(json).map_err(|e| format!("Can not parse events: {}", e))?;
    let events_filtered = events
        .iter()
        .filter(|x| x.created_at > opt.since)
        .filter(|x| x.payload.is_some())
        .collect();

    for (repo, events) in group_by_repos(events_filtered) {
        println!("- {}:", repo);
        let payloads = events.iter().map(|x| x.payload.as_ref().unwrap()).collect();
        for e in convert(&payloads, &opt.user) {
            println!("  * {}", e)
        }
    }

    Ok(())
}

fn main() {
    match run() {
        Ok(_) => (),
        Err(e) => {
            writeln!(&mut stderr(), "{}", e).ok();
            process::exit(1);
        }
    }
}
