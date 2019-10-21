// TODO: figure how to handle prs updates (push)

use chrono::prelude::*;
use reqwest::header::{HeaderMap, AUTHORIZATION, LINK};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use crate::report::*;

// Github response structs

#[derive(Deserialize)]
struct Repo {
    full_name: String,
    source: Option<Box<Repo>>,
}

#[derive(Deserialize)]
struct EventRepo {
    name: String,
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct PullRequest {
    number: u64,
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
    number: u64,
    html_url: String,
    title: String,
    user: User,
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
struct PushPayload {
    r#ref: String,
    #[serde(skip)]
    pull_requests: Option<Vec<PullRequest>>,
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
    #[serde(rename = "PushEvent")]
    Push(PushPayload),
}

#[derive(Deserialize)]
struct Event {
    repo: EventRepo,
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

struct GithubApi<'a> {
    user: &'a str,
    token: &'a str,
}

impl GithubApi<'_> {
    fn events(
        &self,
        since: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
    ) -> Result<Vec<Event>, String> {
        let mut events = Vec::new();
        let mut stop = false;
        let mut page: u8 = 1;
        // call github until event with created_at <= since is found
        // or no more events available
        loop {
            let (page_events, has_next_page) = self.events_page_request(page)?;
            if !has_next_page && !page_events.is_empty() {
                let last_event = &page_events[page_events.len() - 1];
                if last_event.created_at > since {
                    println!(
                        "WARNING: Events since requested date are unavailable. Last event date: {}",
                        last_event.created_at,
                    );
                }
            }

            let events_iter = page_events
                .into_iter()
                .filter(|x| {
                    if x.created_at >= since {
                        true
                    } else {
                        stop = true;
                        false
                    }
                })
                .filter(|x| until.map_or(true, |d| x.created_at < d))
                .filter(|x| x.payload.is_some());

            events.extend(events_iter);

            if stop || !has_next_page {
                break;
            }

            page += 1;
        }

        Ok(events)
    }

    fn get_repo(&self, repo: &str) -> Result<Repo, String> {
        let mut resp = self.request(&format!("https://api.github.com/repos/{}", repo,))?;

        let repo: Repo = resp
            .json()
            .map_err(|e| format!("Can not parse Github response: {}", e))?;

        Ok(repo)
    }

    fn find_prs(&self, repo: &str, head: &str) -> Result<Vec<PullRequest>, String> {
        let mut resp = self.request(&format!(
            "https://api.github.com/repos/{}/pulls?state=all&head={}",
            repo, head,
        ))?;

        let prs: Vec<PullRequest> = resp
            .json()
            .map_err(|e| format!("Can not parse Github response: {}", e))?;

        Ok(prs)
    }

    fn request(&self, url: &str) -> Result<reqwest::Response, String> {
        let resp = reqwest::Client::new()
            .get(url)
            .header(AUTHORIZATION, format!("token {}", self.token))
            .send()
            .map_err(|e| format!("Request to Github failed: {}", e))?
            .error_for_status()
            .map_err(|e| format!("Incorrect response status: {}", e))?;

        Ok(resp)
    }

    fn events_page_request(&self, page: u8) -> Result<(Vec<Event>, bool), String> {
        // documentation says per_page isn't supported but it is :-D
        let mut resp = self.request(&format!(
            "https://api.github.com/users/{}/events?page={}&per_page=100",
            self.user, page,
        ))?;

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

fn convert(
    login: &str,
    issue_comments: bool,
    events: &[&EventPayload],
) -> Result<Vec<Entry>, String> {
    let mut res = HashMap::new();

    for event in events {
        match event {
            EventPayload::PullRequest(p) => {
                let pr = &p.pull_request;
                let entry = res.entry(pr.number).or_insert(Entry {
                    r#type: String::from("PR"),
                    title: pr.title.clone(),
                    url: Some(pr.html_url.clone()),
                    actions: Vec::new(),
                });

                let mut action = p.action.clone();
                if action == "closed" && pr.merged {
                    action = if login != pr.user.login {
                        String::from("reviewed")
                    } else {
                        String::from("merged")
                    }
                }
                // can be pushes before opening a PR, skip them
                if action == "opened" {
                    entry.actions.retain(|x| x != "pushed");
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
                if pr.user.login == login {
                    continue;
                }

                res.entry(pr.number).or_insert(Entry {
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
                if pr.user.login == login {
                    continue;
                }

                res.entry(pr.number).or_insert(Entry {
                    r#type: String::from("PR"),
                    title: pr.title.clone(),
                    url: Some(pr.html_url.clone()),
                    actions: vec![String::from("reviewed")],
                });
            }
            EventPayload::Issue(p) => {
                if p.action != "opened" {
                    continue;
                }

                let issue = &p.issue;
                let entry = res.entry(issue.number).or_insert(Entry {
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
                if p.action != "created" {
                    continue;
                }

                let issue = &p.issue;
                // IssueComment returned for both Issues and Pull Requests
                // in case of PR issue has extra field `pull_request`
                // which is an object with link, but it's easier to check html_url
                let entity_type = issue
                    .html_url
                    .split('/')
                    .nth(5)
                    .expect("url must be parsable");
                if entity_type == "pull" {
                    if issue.user.login == login {
                        continue;
                    }

                    res.entry(issue.number).or_insert(Entry {
                        r#type: String::from("PR"),
                        title: issue.title.clone(),
                        url: Some(issue.html_url.clone()),
                        actions: vec![String::from("reviewed")],
                    });
                    continue;
                }

                if !issue_comments || res.contains_key(&issue.number) {
                    continue;
                }
                res.insert(
                    issue.number,
                    Entry {
                        r#type: String::from("Issue"),
                        title: issue.title.clone(),
                        url: Some(issue.html_url.clone()),
                        actions: vec![String::from("commented")],
                    },
                );
            }
            EventPayload::Push(p) => {
                if let Some(prs) = &p.pull_requests {
                    for pr in prs {
                        // insert Entry only if this PR doesn't exist in the history yet
                        // to avoid pushed actions for just opened PRs
                        res.entry(pr.number).or_insert(Entry {
                            r#type: String::from("PR"),
                            title: pr.title.clone(),
                            url: Some(pr.html_url.clone()),
                            actions: vec![String::from("pushed")],
                        });
                    }
                }
            }
        }
    }

    Ok(res.values().cloned().collect())
}

fn enhance_events(gh: &GithubApi, events: &mut Vec<Event>) -> Result<(), String> {
    // try to find pull requests for push events
    let mut repo_cache = HashMap::new();
    let mut checked_refs = HashSet::new();
    for e in events {
        if let Some(EventPayload::Push(p)) = e.payload.as_mut() {
            // even prs _can_ be opened from master, I don't do that
            // this check allows to skip many pushes that happend because of the merge
            if p.r#ref == "refs/heads/master" {
                continue;
            }

            let repo_name = &e.repo.name;
            if !checked_refs.insert(format!("{}_{}", repo_name, p.r#ref)) {
                continue;
            }
            // events contain only repo name but we need source as well for forks
            let repo = match repo_cache.get(repo_name) {
                Some(r) => &r,
                None => {
                    let r = gh.get_repo(repo_name)?;
                    repo_cache.insert(String::from(repo_name), r);
                    // FIXME there must be better way to do it without violation of lifetime
                    repo_cache.get(repo_name).unwrap()
                }
            };

            let owner = &repo.full_name.split('/').nth(0).unwrap();
            let head = format!("{}:{}", owner, p.r#ref);
            // try to find PR in source repo if push was made to fork
            let prs = if let Some(source) = &repo.source {
                let prs = gh.find_prs(&source.full_name, &head)?;
                // change source of the event to pr's repository
                e.repo.name = source.full_name.clone();
                prs
            // for non-forks try to find in the repo itself
            } else {
                gh.find_prs(&repo.full_name, &head)?
            };
            // TODO: it is possible that PR can be make to a fork

            if !prs.is_empty() {
                p.pull_requests = Some(prs);
            }
        }
    }

    Ok(())
}

pub fn fetch(
    user: &str,
    token: &str,
    since: DateTime<Utc>,
    until: Option<DateTime<Utc>>,
    issue_comments: bool,
) -> Result<HashMap<String, Vec<Entry>>, String> {
    let gh = GithubApi { user, token };

    let mut events: Vec<Event> = gh.events(since, until)?;
    // enrich events with additional information
    enhance_events(&gh, &mut events)?;
    // converting requires events to be sorted by date
    events.sort_by_key(|x| x.created_at);

    let mut result = HashMap::new();
    for (repo, events) in group_by_repos(&events) {
        let payloads: Vec<&EventPayload> = events
            .into_iter()
            .map(|x| x.payload.as_ref())
            .flatten()
            .collect();

        let events = convert(user, issue_comments, &payloads)?;

        if !events.is_empty() {
            result.insert(repo.clone(), events);
        }
    }

    Ok(result)
}
