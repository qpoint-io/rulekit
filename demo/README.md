# Rulekit Demo

Vue/Vite demo for Rulekit v2. The browser talks to the Go parser, formatter,
rewriter, evaluator, and trace model through a demo-local WASM bridge.

## Run

```sh
npm install
npm run dev
```

`npm run dev` builds `public/rulekit.wasm` and copies Go's `wasm_exec.js` before
starting Vite.

## Layout

- `wasm/`: demo-local Go WASM bridge and build script.
- `public/`: generated WASM artifacts and static assets.
- `src/`: Vue app, components, styles, and TS WASM wrapper.
