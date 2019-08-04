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
use reqwest::header::{HeaderMap, AUTHORIZATION, LINK};
use serde::Deserialize;
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
struct PullRequestPayload {
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
struct IssuePayload {
    action: String,
    issue: Issue,
}

#[derive(Deserialize)]
struct IssueCommentPayload {
    action: String,
    issue: Issue,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "payload")]
enum EventPayload {
    #[serde(rename = "PullRequestEvent")]
    PullRequest(PullRequestPayload),
    #[serde(rename = "PullRequestReviewEvent")]
    Review(PullRequestReviewPayload),
    #[serde(rename = "PullRequestReviewCommentEvent")]
    ReviewComment(PullRequestReviewCommentPayload),
    #[serde(rename = "IssuesEvent")]
    Issue(IssuePayload),
    #[serde(rename = "IssueCommentEvent")]
    IssueComment(IssueCommentPayload),
}

#[derive(Deserialize)]
struct Event {
    repo: Repo,
    #[serde(flatten)]
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

fn group_by_repos(events: &[Event]) -> HashMap<&String, Vec<&Event>> {
    let mut res = HashMap::new();

    for e in events {
        let v = res.entry(&e.repo.name).or_insert_with(Vec::new);
        v.push(e);
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

// github api

// typed link header isn't implemented in headers 0.2.1
struct LinkHeader {
    next: Option<String>,
}

impl LinkHeader {
    fn from_str(v: &str) -> LinkHeader {
        for item in v.split(',') {
            let parts: Vec<&str> = item.splitn(2, ';').map(|x| x.trim()).collect();
            if parts[1] != "rel=\"next\"" {
                continue;
            }
            let a: &str = &parts[0][1..parts[0].len() - 1];
            return LinkHeader {
                next: Some(String::from(a)),
            };
        }
        LinkHeader { next: None }
    }
}

struct GithubEvents {
    user: String,
    token: String,
    since: DateTime<Utc>,
    until: Option<DateTime<Utc>>,
}

impl GithubEvents {
    fn get(&self) -> Result<Vec<Event>, String> {
        let mut events = Vec::new();
        let mut stop = false;
        let mut page: u8 = 1;
        // call github until event with created_at <= since is found
        // or no more events available
        loop {
            let (page_events, has_next_page) = self.page_request(page)?;
            if !has_next_page && !page_events.is_empty() {
                let last_event = &page_events[page_events.len() - 1];
                if last_event.created_at > self.since {
                    println!(
                        "WARNING: Events since requested date are unavailable. Last event date: {}",
                        last_event.created_at,
                    );
                }
            }

            let events_iter = page_events
                .into_iter()
                .filter(|x| {
                    if x.created_at >= self.since {
                        true
                    } else {
                        stop = true;
                        false
                    }
                })
                .filter(|x| self.until.map_or(true, |d| x.created_at < d))
                .filter(|x| x.payload.is_some());

            events.extend(events_iter);

            if stop || !has_next_page {
                break;
            }

            page += 1;
        }

        Ok(events)
    }

    fn page_request(&self, page: u8) -> Result<(Vec<Event>, bool), String> {
        // documentation says per_page isn't supported but it is :-D
        let mut resp = reqwest::Client::new()
            .get(&format!(
                "https://api.github.com/users/{}/events?page={}&per_page=100",
                self.user, page,
            ))
            .header(AUTHORIZATION, format!("token {}", self.token))
            .send()
            .map_err(|e| format!("Request to Github failed: {}", e))?
            .error_for_status()
            .map_err(|e| format!("Incorrect response status: {}", e))?;

        let events: Vec<Event> = resp
            .json()
            .map_err(|e| format!("Can not parse Github response: {}", e))?;

        Ok((events, Self::has_next_page(resp.headers())))
    }

    fn has_next_page(headers: &HeaderMap) -> bool {
        let link = match headers.get(LINK) {
            Some(link) => link,
            None => return false,
        };
        let link_str = match link.to_str() {
            Ok(link_str) => link_str,
            Err(_) => return false,
        };
        let next_url = LinkHeader::from_str(link_str).next;
        next_url.is_some()
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let opt = Opt::from_args();

    let gh = GithubEvents {
        user: opt.user.clone(),
        token: opt.token.clone(),
        since: opt.since,
        until: opt.until,
    };
    let events = gh.get()?;

    let c = Convertor {
        login: opt.user,
        issue_comments: opt.issue_comments,
    };

    for (repo, events) in group_by_repos(&events) {
        println!("* {}:", repo);
        let payloads: Vec<&EventPayload> = events
            .into_iter()
            .map(|x| x.payload.as_ref())
            .flatten()
            .collect();
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
