# Agents

Run `make ci` at the start of any task, to know the baseline, and again before every commit or push. It must pass before you commit; it runs the same checks as GitHub Actions (`.github/workflows/ci.yml`).

CI pins Rust 1.97.1 because the trybuild compile-fail tests compare compiler messages. If those tests fail locally with only message differences, check your toolchain version before changing the `.stderr` files.
