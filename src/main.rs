use std::error::Error;
use std::io::{stderr, Write};
use std::process;

use chrono::prelude::*;
use structopt::StructOpt;
use time::Duration;

mod gcalendar;
mod github;
mod report;

// Cli
#[derive(StructOpt)]
#[structopt(
    name = "standup-rs",
    about = "Generate a report for morning standup using Github."
)]
struct Opt {
    #[structopt(long, env = "STANDUP_LOGIN", empty_values = false)]
    /// Github user login
    github_user: String,

    #[structopt(long, env = "STANDUP_GITHUB_TOKEN", empty_values = false)]
    /// Personal Github token
    github_token: String,

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

    #[structopt(long, env = "STANDUP_CALENDAR_CLIENT_ID", default_value = "")]
    // Google Calendar client_id
    calendar_client_id: String,

    // FIXME validate that both id&secret are presented always
    #[structopt(long, env = "STANDUP_CALENDAR_CLIENT_SECRET", default_value = "")]
    // Google Calendar client_secret
    calendar_client_secret: String,

    #[structopt(long, env = "STANDUP_CALENDAR_ID", default_value = "")]
    /// Google Calendar ID
    calendar_id: String,
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

    // FIXME google calendar stuff must be optional
    let mut c = gcalendar::Calendar::new(&opt.calendar_client_id, &opt.calendar_client_secret);

    if !c.authorized() {
        println!("Please visit the url to authorize the application");
        println!("{}", c.authorize_url());

        c.listen_for_code()?;
    }

    if opt.calendar_client_id != "" && opt.calendar_id == "" {
        let calendars = c.list()?;
        println!("Google Calendar token found but calendar-id is unset");
        println!("Available calendars:");
        for cal in calendars {
            println!("[{}]: {}", cal.id, cal.summary)
        }
        println!(
            "Set calendar id into STANDUP_CALENDAR_ID env var or pass it as --calendar-id flag"
        );

        return Ok(());
    }

    if opt.calendar_id != "" {
        let events = c.events(opt.since, opt.until, opt.calendar_id)?;
        for e in events {
            println!("* {}", e);
        }
    }

    let grouped_events = github::fetch(
        &opt.github_user,
        &opt.github_token,
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
