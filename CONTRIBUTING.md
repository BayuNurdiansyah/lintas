# Contributing

1. Fork the repo and branch off `main`.
2. Before committing: `cargo fmt && cargo clippy --release -- -D warnings && cargo build --release && cargo test --release`.
3. Test with at least two machines (or one machine plus a VM), and mention the distro and desktop you tested in the PR.
4. Always keep a way out while testing: `Ctrl+Alt+Shift+Esc`, or run it over SSH so you can kill it if input gets stuck.
5. If you're touching `src/tls.rs` (pairing, encryption, trust storage), say so explicitly in the PR description and explain the security reasoning, not just what changed. That file has already had one serious authentication bug found and fixed (see CLAUDE.md's history if you want the details); changes there get read more carefully than the rest of the codebase.
