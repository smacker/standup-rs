# Standup-rs

```
Generate a report for morning standup using Github.

USAGE:
    standup_rs [FLAGS] [OPTIONS] --token <token> --user <user>

FLAGS:
    -h, --help              Prints help information
        --issue-comments    Add issues with comments into a report
    -V, --version           Prints version information

OPTIONS:
    -s, --since <since>    Valid values: yesterday, friday, today, yyyy-mm-dd [default: yesterday]
    -t, --token <token>    Personal Github token [env: STANDUP_GITHUB_TOKEN=]
    -u, --until <until>    Valid values: today, yyyy-mm-dd
    -l, --user <user>      Github user login [env: STANDUP_LOGIN=]
```

### Example output

```
$ ./standup_rs -s today --issue-comments
* src-d/sourced-ui:
  - [Issue] (commented) Improve contributors charts https://github.com/src-d/sourced-ui/issues/194
  - [PR] (merged) Cherry-pick: allow user re-order top-level tabs https://github.com/src-d/sourced-ui/pull/234
  - [PR] (opened) Fix formatter for metadata progress chart https://github.com/src-d/sourced-ui/pull/236
  - [PR] (opened) Improve contributors charts https://github.com/src-d/sourced-ui/pull/237
* src-d/sourced-ce:
  - [PR] (opened) Exclude forks by default https://github.com/src-d/sourced-ce/pull/185
  - [PR] (reviewed) Separates local and orgs workdirs https://github.com/src-d/sourced-ce/pull/183
  - [PR] (merged) Update .env file on init always https://github.com/src-d/sourced-ce/pull/181
  - [PR] (reviewed) Adds monitoring of services when opening ui to eventually fail fast https://github.com/src-d/sourced-ce/pull/186
  - [Issue] (commented) Separates local and orgs workdirs https://github.com/src-d/sourced-ce/pull/183
* src-d/ghsync:
  - [Issue] (commented) Add no-forks option https://github.com/src-d/ghsync/pull/60
  - [PR] (merged) Add no-forks option https://github.com/src-d/ghsync/pull/60
```
