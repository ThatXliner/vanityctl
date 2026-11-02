# Working on vanityctl

For live service operations, read [the operations skill](skills/vanityctl/SKILL.md).
Use focused context, discovery and targeted lifecycle commands before writing
host-specific wrappers. Keep private registry contents and chat transcripts out
of this public repository.

Verify Rust changes with `cargo fmt --check`, `cargo test --locked` and
`cargo clippy --all-targets --locked -- -D warnings`. Backend regression tests use
temporary configuration and RecordingRunner; live integration fixtures must own
their resource names and clean up only those resources.

When you say you'll implement something, actually do it.
When working on UI, ask Claude to design the interface.
