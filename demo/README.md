# Rulekit Demo

Browser playground for Rulekit v2: edit a rule, inspect and edit its parsed
structure, and evaluate it against JSON input with a step-by-step trace. Built
with React, Vite, Tailwind, and [shadcn/ui](https://ui.shadcn.com) (Base UI
primitives; light and dark themes, press `d` to toggle). The browser talks to
the Rust crate's parser, formatter, rewriter, evaluator, and trace model
through a demo-local WASM bridge built with
[wasm-bindgen](https://wasm-bindgen.github.io/wasm-bindgen/).

## Prerequisites

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
```

The CLI version must equal the `wasm-bindgen` version pinned in
`wasm/Cargo.toml`; `wasm/build.sh` checks this.

## Run

```sh
npm install
npm run dev
```

`npm run dev` (and `npm run build`) first runs `wasm/build.sh`, which compiles
the bridge for `wasm32-unknown-unknown` and generates `src/wasm/` (an ES
module, its type declarations, and `rulekit_bg.wasm`).

## Layout

- `wasm/`: demo-local Rust WASM bridge crate and build script.
- `src/wasm/`: generated bridge module (not committed).
- `src/lib/rulekit.ts`: typed wrapper over the bridge's JSON API.
- `src/hooks/use-playground.ts`: playground state and actions.
- `src/components/`: editor, structure tree, inspector, and trace panels.
- `src/components/ui/`: shadcn components (`npx shadcn@latest add <name>`).
