use std::error::Error;
use std::io::{stderr, Write};
use std::process;

use chrono::prelude::*;
use structopt::StructOpt;
use time::Duration;

mod github;
mod report;

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

    let grouped_events = github::fetch(
        &opt.user,
        &opt.token,
        opt.since,
        opt.until,
        opt.issue_comments,
    )?;

    for (repo, events) in grouped_events {
        println!("* {}:", repo);
        for e in events {
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
