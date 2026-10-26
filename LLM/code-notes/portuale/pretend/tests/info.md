# crates/portuale/src/pretend/tests/info.rs
`emerge --info` field pins (#504).

## fn repo_timestamp_is_the_first_grabfile_entry
- `timestamp:` takes `portage.grabfile(...)[0].strip()` (#504): comments and blank lines are skipped, trailing blanks stripped.

## fn repo_head_commit_is_git_only
- Only a `sync-type = git` repo reports `head commit:` (the git sync module's `retrieve_head`) (#504); rsync raises NotImplementedError in Portage and prints nothing.

## fn info_masks_url_passwords_like_hide_url_passwd
- `sync-uri` of repos and binrepos goes through `_hide_url_passwd` (#504).
