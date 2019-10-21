use std::error::Error;
use std::io::{self, stderr, BufRead, Write};
use std::path::Path;
use std::process;

use chrono::prelude::*;
use dirs::home_dir;
use structopt::StructOpt;
use time::Duration;

mod config;
mod gcalendar;
mod github;
mod report;

use self::config::Config;

// Cli
#[derive(StructOpt)]
#[structopt(
    name = "standup-rs",
    about = "Generate a report for morning standup using GitHub and Google Calendar."
)]
struct Opt {
    #[structopt(
        short = "s",
        long,
        default_value = "yesterday",
        parse(try_from_str = parse_since)
    )]
    /// Valid values: yesterday, friday, today, yyyy-mm-dd
    since: DateTime<Utc>,

    #[structopt(short = "u", long, parse(try_from_str = parse_until))]
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

fn ask(question: &str) -> String {
    let mut answer = String::new();

    loop {
        println!("{}:", question);
        print!("> ");
        io::stdout().flush().unwrap();
        io::stdin()
            .lock()
            .read_line(&mut answer)
            .expect("couldn't read from stdio");

        answer = answer.trim().to_owned();
        if !answer.is_empty() {
            break;
        }
    }

    answer
}

const YES_ANSWERS: [&str; 3] = ["y", "yes", "yep"];
const NO_ANSWERS: [&str; 3] = ["n", "no", "nope"];

fn ask_yes_no(question: &str) -> bool {
    let mut answer = String::new();
    loop {
        print!("{} (Y/N): ", question);
        io::stdout().flush().unwrap();
        io::stdin()
            .lock()
            .read_line(&mut answer)
            .expect("couldn't read from stdio");

        answer = answer.trim().to_lowercase();
        if YES_ANSWERS.iter().any(|x| x == &answer) {
            return true;
        }
        if NO_ANSWERS.iter().any(|x| x == &answer) {
            return false;
        }
    }
}

fn wizard() -> Result<Config, String> {
    println!("Standup-rs requires access tokens to generate reports.");
    let github_username = ask("Enter your github username");
    println!("Go to https://github.com/settings/tokens to obtain personal access token.");
    let github_token = ask("Enter github token");

    // TODO validate the token & username here

    let mut cfg = Config {
        github: config::Github {
            username: github_username,
            token: github_token,
        },
        google_client: None,
        google_token: None,
        gcal: None,
    };

    if ask_yes_no("Do you want to connect Google Calendar?") {
        println!("To obtain the token follow the instructions:");
        println!("- Go to the Google developer console: https://console.developers.google.com/");
        println!("- Make a new project");
        println!("- In the menu go to APIs & Services");
        println!("- At the top of the page click Enable APIs and Services");
        println!("- Enable the Calendar API");
        println!("- On the sidebar click Credentials");
        println!("- Create a new OAuth Client ID. Set the Application type to Other.");
        println!("- Fill the consent form. Anything optional fields can be left blank.");
        println!("- Go back to the credentials page and get Client ID and Client secret");

        let client_id = ask("Enter your Google Client ID");
        let client_secret = ask("Enter your Google Client Secret");

        cfg.google_client = Some(config::GoogleClient {
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
        });

        // run auth & choose calendar id flow

        let c = gcalendar::Calendar::new(&cfg);
        println!("Please visit the url to authorize the application");
        println!("{}", c.authorize_url());
        cfg.google_token = Some(c.listen_for_code());

        let c = gcalendar::Calendar::new(&cfg);
        let calendars = c.list()?;
        println!("Available calendars:");
        for (i, cal) in calendars.iter().enumerate() {
            println!("[{}]: {}", i + 1, cal.summary)
        }
        let cal_n_str = ask("Choose the calendar to use");
        let cal_n: usize = cal_n_str
            .parse()
            .map_err(|_| format!("incorrect value: {}", cal_n_str))?;

        if cal_n > calendars.len() || cal_n < 1 {
            return Err(format!("incorrect value: {}", cal_n_str));
        }

        cfg.gcal = Some(config::GoogleCalendar {
            id: calendars[cal_n - 1].id.clone(),
        });
    };

    Ok(cfg)
}

fn run() -> Result<(), Box<dyn Error>> {
    let opt = Opt::from_args();
    let config_path = Path::join(&home_dir().unwrap(), ".standup");
    let mut cfg = match Config::load(&config_path)? {
        Some(c) => c,
        None => {
            let c = wizard()?;
            c.save(&config_path)?;
            c
        }
    };

    if cfg.gcal.is_some() {
        // FIXME I have to re-create client after checking for new token
        // because I can't mutate an object that is already borrowed (it may cause race condition)
        // can it be solved with different life-time for cfg inside calendar?
        // or do I need to refactor it somehow?
        {
            let c = gcalendar::Calendar::new(&cfg);
            let new_token = c.refresh_if_needed()?;
            if new_token.is_some() {
                cfg.google_token = new_token;
                cfg.save(&config_path)?;
            }
        };
        let c = gcalendar::Calendar::new(&cfg);
        let events = c.events(opt.since, opt.until)?;
        for e in events {
            println!("* {}", e);
        }
    }

    let grouped_events = github::fetch(
        &cfg.github.username,
        &cfg.github.token,
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
