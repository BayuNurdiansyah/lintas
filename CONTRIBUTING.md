# Contributing

1. Fork the repo and branch off `main`.
2. Before committing: `cargo fmt && cargo clippy --release -- -D warnings && cargo build --release`.
3. Test with at least two machines (or one machine plus a VM), and mention the distro and desktop you tested in the PR.
4. Always keep a way out while testing: `Ctrl+Alt+Shift+Esc`, or run it over SSH so you can kill it if input gets stuck.
