# Standup-rs

Generate a report for morning standup using GitHub and Google Calendar APIs.

## Features

- Supported GitHub events:
    - PRs: opened, merged, reviewed
    - Issues: opened, commented (optional, disable by default)
- Support for accepted events in Google Calendar (optional)
- Shortcuts for --since flag
- Copy-paste-able output for Slack
- Private repos are analyzed as well
- Simple step-by-step setup

## Install

Go to [releases page](https://github.com/smacker/standup-rs/releases) and download a binary for your platform.

## Usage

```
USAGE:
    standup_rs [FLAGS] [OPTIONS]

FLAGS:
    -h, --help              Prints help information
        --issue-comments    Add issues with comments into a report
    -V, --version           Prints version information

OPTIONS:
    -s, --since <since>    Valid values: yesterday, friday, today, yyyy-mm-dd [default: yesterday]
    -u, --until <until>    Valid values: today, yyyy-mm-dd
```

On the first run it will guide you how to obtain necessary tokens and save configuration into `~/.standup`.

## Example output

```
$ ./standup_rs --since "2019-08-06" --until "2019-08-07"
* [Meeting] Apps Team Focus
* [Meeting] Engineering Demo
* src-d/ghsync:
  - [PR] (opened) Add tests for RateLimitTransport https://github.com/src-d/ghsync/pull/61
* src-d/sourced-ce:
  - [PR] (merged) Limit container resources https://github.com/src-d/sourced-ce/pull/182
  - [PR] (merged) Fix workdirs sub-command without active workdir https://github.com/src-d/sourced-ce/pull/190
  - [PR] (opened) Disallow forks flag switch https://github.com/src-d/sourced-ce/pull/196
  - [PR] (merged, opened) Rename workdir.WorkdirType to workdir.Type https://github.com/src-d/sourced-ce/pull/193
* src-d/sourced-ui:
  - [PR] (merged) Improve contributors charts https://github.com/src-d/sourced-ui/pull/237
```
