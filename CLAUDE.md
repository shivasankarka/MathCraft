# CLAUDE.md

Read `AGENTS.md` first; it is the source of truth for rules. Then `docs/PLAN.md` (plan and decision log) and
`docs/STATUS.md` (what works). The ArtCraft rules we follow are under `reference/`.

Quick commands:

```sh
cargo run --release -p mathcraft                 # desktop app (optionally: -- examples/tour.py)
cargo run -p mathcraft-cli -- eval 'diff(sin(x), x)'
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check
```
