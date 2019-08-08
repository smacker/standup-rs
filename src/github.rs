// TODO: figure how to handle prs updates (push)

use chrono::prelude::*;
use reqwest::header::{HeaderMap, AUTHORIZATION, LINK};
use serde::Deserialize;
use std::collections::HashMap;

#[path = "report.rs"]
mod report;
use report::*;

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

// helpers

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

// github api

struct GithubEvents<'a> {
    user: &'a str,
    token: &'a str,
    since: DateTime<Utc>,
    until: Option<DateTime<Utc>>,
}

impl GithubEvents<'_> {
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

// Transformations

fn group_by_repos(events: &[Event]) -> HashMap<&String, Vec<&Event>> {
    let mut res = HashMap::new();

    for e in events {
        let v = res.entry(&e.repo.name).or_insert_with(Vec::new);
        v.push(e);
    }

    res
}

pub struct Convertor<'a> {
    login: &'a str,
    issue_comments: bool,
}

impl Convertor<'_> {
    fn convert(&self, events: &[&EventPayload]) -> Vec<Entry> {
        let mut res = HashMap::new();

        for event in events {
            match event {
                EventPayload::PullRequest(p) => {
                    let pr = &p.pull_request;
                    let entry = res.entry(pr.id).or_insert(Entry {
                        r#type: String::from("PR"),
                        title: pr.title.clone(),
                        url: Some(pr.html_url.clone()),
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
                        url: Some(pr.html_url.clone()),
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
                        url: Some(pr.html_url.clone()),
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
                        url: Some(issue.html_url.clone()),
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
                            url: Some(issue.html_url.clone()),
                            actions: vec![String::from("commented")],
                        },
                    );
                }
            }
        }

        res.values().cloned().collect()
    }
}

pub fn fetch(
    user: &str,
    token: &str,
    since: DateTime<Utc>,
    until: Option<DateTime<Utc>>,
    issue_comments: bool,
) -> Result<HashMap<String, Vec<Entry>>, String> {
    let gh = GithubEvents {
        user,
        token,
        since,
        until,
    };

    let c = Convertor {
        login: user,
        issue_comments,
    };

    let events = gh.get()?;
    let mut result = HashMap::new();

    for (repo, events) in group_by_repos(&events) {
        let payloads: Vec<&EventPayload> = events
            .into_iter()
            .map(|x| x.payload.as_ref())
            .flatten()
            .collect();

        result.insert(repo.clone(), c.convert(&payloads));
    }

    Ok(result)
}
