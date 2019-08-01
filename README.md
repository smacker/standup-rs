# Standup-rs

```
Generate a report for morning standup using Github.

USAGE:
    standup_rs [OPTIONS] --token <token> --user <user>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -s, --since <since>    Valid values: yesterday, friday, today, yyyy-mm-dd [default: yesterday]
    -t, --token <token>    Personal Github token [env: STANDUP_GITHUB_TOKEN=]
    -u, --user <user>      Github user login [env: STANDUP_USER=]
```

### Example output

```
- src-d/sourced-ui:
 * [PR] (merged) Sort lists in exported dashboards https://github.com/src-d/sourced-ui/pull/224
 * [PR] (merged) Use utf8 encoding for gitbase connection https://github.com/src-d/sourced-ui/pull/233
 * [PR] (merged, opened) Remove changed_on field from dashboard export https://github.com/src-d/sourced-ui/pull/232
- src-d/sourced-ce:
 * [PR] (merged, opened) Fix incorrect example in documentation https://github.com/src-d/sourced-ce/pull/176
 * [PR] (reviewed) Add bblfsh calls to end-to-end tests https://github.com/src-d/sourced-ce/pull/175
```