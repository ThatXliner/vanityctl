# Working on vanityctl

For a new host or migration of existing services, start with
[prompts/install.md](prompts/install.md) and [docs/onboarding.md](docs/onboarding.md).
For live service operations, read [the operations skill](skills/vanityctl/SKILL.md).
Use focused context, discovery and targeted lifecycle commands before writing
host-specific wrappers. Keep private registry contents and chat transcripts out
of this public repository.

Verify Rust changes with `cargo fmt --check`, `cargo test --locked` and
`cargo clippy --all-targets --locked -- -D warnings`. Backend regression tests use
temporary configuration and RecordingRunner; live integration fixtures must own
their resource names and clean up only those resources.

Finish the changes you describe. If you stop short, say what remains.
When working on UI, ask Claude to design the interface.
