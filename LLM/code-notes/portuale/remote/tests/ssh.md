# crates/portuale/src/remote/tests/ssh.rs
Context moved out of the code comments.

## fn local_client_bash_exports_the_binary_and_scrubs_the_servers_vdb_selection
- Backlog numbers removed from the comment (original text): #313: a local-transport client script gets `PORTUALE_BIN` (the shim's target) and has the server's `PORTUALE_VDB_*` removed from its inherited environment (`get_envs` reports a removal as `None`).
