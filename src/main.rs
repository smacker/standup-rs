// Supported events:
//
// PullRequestEvent - action: open/merged
// IssuesEvent - action: open
// PullRequestReviewEvent
// PullRequestReviewCommentEvent (merged with review event)
// IssueCommentEvent (optional, disabled by default)
//
// TODO: figure how to handle prs updates (push)

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io::{stderr, Write};
use std::process;

use chrono::prelude::*;
use reqwest::header::AUTHORIZATION;
use serde::{Deserialize, Deserializer};
use structopt::StructOpt;
use time::Duration;

// Github response structs

#[derive(Deserialize)]
struct Repo {
    name: String,
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct PullRequest {
    id: u64,
    html_url: String,
    title: String,
    #[serde(default)]
    merged: bool,
    user: User,
}

#[derive(Deserialize)]
struct PullRequestEventPayload {
    action: String,
    pull_request: PullRequest,
}

#[derive(Deserialize)]
struct Issue {
    id: u64,
    html_url: String,
    title: String,
}

#[derive(Deserialize)]
struct PullRequestReviewPayload {
    action: String,
    pull_request: PullRequest,
}

#[derive(Deserialize)]
struct PullRequestReviewCommentPayload {
    action: String,
    pull_request: PullRequest,
}

#[derive(Deserialize)]
struct IssuesEventPayload {
    action: String,
    issue: Issue,
}

#[derive(Deserialize)]
struct IssueCommentPayload {
    action: String,
    issue: Issue,
}

#[derive(Deserialize)]
enum EventPayload {
    PullRequest(PullRequestEventPayload),
    Review(PullRequestReviewPayload),
    ReviewComment(PullRequestReviewCommentPayload),
    Issue(IssuesEventPayload),
    IssueComment(IssueCommentPayload),
}

struct Event {
    repo: Repo,
    payload: Option<EventPayload>,
    created_at: DateTime<Utc>,
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct EventHelper {
            r#type: String,
            repo: Repo,
            created_at: DateTime<Utc>,
            payload: serde_json::Value,
        }

        let helper = EventHelper::deserialize(deserializer)?;
        let payload = match helper.r#type.as_ref() {
            "PullRequestEvent" => {
                let p = PullRequestEventPayload::deserialize(helper.payload)
                    .map_err(serde::de::Error::custom)?;
                Some(EventPayload::PullRequest(p))
            }
            "IssuesEvent" => {
                let p = IssuesEventPayload::deserialize(helper.payload)
                    .map_err(serde::de::Error::custom)?;
                Some(EventPayload::Issue(p))
            }
            "PullRequestReviewEvent" => {
                let p = PullRequestReviewPayload::deserialize(helper.payload)
                    .map_err(serde::de::Error::custom)?;
                Some(EventPayload::Review(p))
            }
            "PullRequestReviewCommentEvent" => {
                let p = PullRequestReviewCommentPayload::deserialize(helper.payload)
                    .map_err(serde::de::Error::custom)?;
                Some(EventPayload::ReviewComment(p))
            }
            "IssueCommentEvent" => {
                let p = IssueCommentPayload::deserialize(helper.payload)
                    .map_err(serde::de::Error::custom)?;
                Some(EventPayload::IssueComment(p))
            }
            _ => None,
        };

        Ok(Event {
            repo: helper.repo,
            created_at: helper.created_at,
            payload,
        })
    }
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

fn group_by_repos<'a>(events: &[&'a Event]) -> HashMap<&'a String, Vec<&'a Event>> {
    let mut res = HashMap::new();

    for e in events {
        let v = res.entry(&e.repo.name).or_insert_with(Vec::new);
        v.push(*e);
    }

    res
}

struct Convertor {
    login: String,
    issue_comments: bool,
}

impl Convertor {
    fn convert(&self, events: &[&EventPayload]) -> Vec<Entry> {
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
                EventPayload::Review(p) => {
                    if p.action != "submitted" {
                        continue;
                    }

                    let pr = &p.pull_request;
                    if pr.user.login == self.login {
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
                    if p.action != "created" {
                        continue;
                    }

                    let pr = &p.pull_request;
                    if pr.user.login == self.login {
                        continue;
                    }

                    res.entry(pr.id).or_insert(Entry {
                        r#type: String::from("PR"),
                        title: pr.title.clone(),
                        url: pr.html_url.clone(),
                        actions: vec![String::from("reviewed")],
                    });
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
                EventPayload::IssueComment(p) => {
                    if !self.issue_comments || p.action != "created" {
                        continue;
                    }

                    let issue = &p.issue;
                    if res.contains_key(&issue.id) {
                        continue;
                    }
                    res.insert(
                        issue.id,
                        Entry {
                            r#type: String::from("Issue"),
                            title: issue.title.clone(),
                            url: issue.html_url.clone(),
                            actions: vec![String::from("commented")],
                        },
                    );
                }
            }
        }

        res.values().cloned().collect()
    }
}

// Cli
#[derive(StructOpt)]
#[structopt(
    name = "standup-rs",
    about = "Generate a report for morning standup using Github."
)]
struct Opt {
    #[structopt(short = "l", long, env = "STANDUP_LOGIN", empty_values = false)]
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
    /// Valid values: yesterday, friday, today, yyyy-mm-dd
    since: DateTime<Utc>,

    #[structopt(short = "u", long, parse(try_from_str = "parse_until"))]
    /// Valid values: today, yyyy-mm-dd
    until: Option<DateTime<Utc>>,

    #[structopt(long = "issue-comments")]
    /// Add issues with comments into a report
    issue_comments: bool,
}

fn parse_date(v: &str) -> Result<Date<Local>, &str> {
    NaiveDate::parse_from_str(v, "%Y-%m-%d")
        .map(|v| Local.from_local_date(&v).earliest().unwrap())
        .map_err(|_| "unsupported value")
}

fn parse_since(v: &str) -> Result<DateTime<Utc>, &str> {
    let d = match v {
        "yesterday" => Local::today() - Duration::days(1),
        "friday" => {
            let mut r = Local::today();
            while r.weekday() != Weekday::Fri {
                r = r - Duration::days(1);
            }
            r
        }
        "today" => Local::today(),
        _ => parse_date(v)?,
    };

    Ok(DateTime::from(d.and_hms(0, 0, 0)))
}

fn parse_until(v: &str) -> Result<DateTime<Utc>, &str> {
    let d = match v {
        "today" => Local::today(),
        _ => parse_date(v)?,
    };

    Ok(DateTime::from(d.and_hms(0, 0, 0)))
}

fn run() -> Result<(), Box<dyn Error>> {
    let opt = Opt::from_args();

    // documentation says per_page isn't supported but it is :-D
    // TODO add pagination
    let events: Vec<Event> = reqwest::Client::new()
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

    let events_filtered: Vec<&Event> = events
        .iter()
        .filter(|x| x.created_at >= opt.since)
        .filter(|x| opt.until.map_or(true, |d| x.created_at < d))
        .filter(|x| x.payload.is_some())
        .collect();

    let c = Convertor {
        login: opt.user,
        issue_comments: opt.issue_comments,
    };

    for (repo, events) in group_by_repos(&events_filtered) {
        println!("* {}:", repo);
        let payloads: Vec<&EventPayload> =
            events.iter().map(|x| x.payload.as_ref().unwrap()).collect();
        for e in c.convert(&payloads) {
            println!("  - {}", e)
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
