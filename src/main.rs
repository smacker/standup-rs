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
use std::env;
use std::fmt;

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
struct PullRequest {
    id: u64,
    html_url: String,
    title: String,
    #[serde(default)]
    merged: bool,
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

fn convert(events: &Vec<&EventPayload>) -> Vec<Entry> {
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
            // FIXME do only when author != login
            EventPayload::Review(p) => {
                let pr = &p.pull_request;
                res.entry(pr.id).or_insert(Entry {
                    r#type: String::from("PR"),
                    title: pr.title.clone(),
                    url: pr.html_url.clone(),
                    actions: vec![String::from("reviewed")],
                });
            }
            EventPayload::ReviewComment(p) => {
                let pr = &p.pull_request;
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
    #[structopt(short = "u", long, default_value = "$STANDUP_USER")]
    /// Github user login
    user: String,

    #[structopt(short = "t", long, default_value = "$STANDUP_GITHUB_TOKEN")]
    /// Personal Github token
    token: String,

    #[structopt(short = "s", long, default_value = "yesteday")]
    /// Non-default values are not implemented
    since: String,
}

fn main() {
    // FIXME replace panics with normal error and exit code

    // FIXME move it to Opt method
    let mut opt = Opt::from_args();
    if opt.token == "$STANDUP_GITHUB_TOKEN" {
        opt.token = env::var("STANDUP_GITHUB_TOKEN").expect("Can not read STANDUP_GITHUB_TOKEN");
    }
    if opt.user == "$STANDUP_USER" {
        opt.user = env::var("STANDUP_USER").expect("Can not read STANDUP_USER");
    }
    // TODO support other values
    let yesteday = Local::today() - Duration::days(1);
    let since = yesteday.and_hms(0, 0, 0);

    // documentation says per_page isn't supported but it is :-D
    // TODO add pagination
    let json: Value = reqwest::Client::new()
        .get(&format!(
            "https://api.github.com/users/{}/events?per_page=100",
            opt.user
        ))
        .header(AUTHORIZATION, format!("token {}", opt.token))
        .send()
        .expect("Request to Github failed")
        .error_for_status()
        .expect("Incorrect response status")
        .json()
        .expect("Can not parse Github response");

    let events = value_to_events(json).unwrap();
    let events_filtered = events
        .iter()
        .filter(|x| x.created_at > DateTime::from(since))
        .filter(|x| x.payload.is_some())
        .collect();

    for (repo, events) in group_by_repos(events_filtered) {
        println!("- {}:", repo);
        let payloads = events.iter().map(|x| x.payload.as_ref().unwrap()).collect();
        for e in convert(&payloads) {
            println!("  * {}", e)
        }
    }
}
