# Rulekit V2 Demo App Plan

This plan describes a Vue demo app that showcases Rulekit v2 as a hybrid text and visual rule editor. The demo should use the Go implementation through WASM so the browser experience exercises the same parser, formatter, rewriter, evaluator, and trace model as the Go package.

## Goals

- Show v2's editor-oriented architecture: text source, AST, formatting, source-preserving rewrite, and evaluation trace.
- Provide a two-pane editor where the text expression and visual AST stay synchronized.
- Preserve the retro CRT/pixel aesthetic from the `rulekit-js` branch and the shared screenshots.
- Keep all demo-specific WASM build code, generated WASM artifacts, JS glue, assets, and app code contained inside the demo directory.
- Build a focused initial GUI editor: rewrite and delete existing rule parts, but do not support adding new nodes yet.
- Render evaluation results as an annotated visual AST near the result card, color-coded per branch status.

## Non-Goals For Initial Demo

- No macro UI or macro examples.
- No add-node workflow in the visual editor.
- No full schema-aware field picker.
- No syntax highlighting until the base editor and rewrite loop work.
- No production WASM packaging story outside the demo directory.
- No attempt to port the old `rulekit-js` app directly; reuse its styling/assets as inspiration and source material.

## Directory Shape

Use a new demo directory, tentatively:

```text
demo/vue/
  README.md
  package.json
  vite.config.ts
  tsconfig.json
  index.html
  wasm/
    main.go
    build.sh
    wasm_exec.js
  public/
    rulekit.wasm
    wasm_exec.js
    assets copied/adapted from rulekit-js
  src/
    main.ts
    App.vue
    components/
    lib/
    styles/
```

All WASM-specific files should live under `demo/vue/wasm` or `demo/vue/public`. Do not add root-level `cmd/wasm`, root `Makefile` targets, or root WASM docs unless we later decide the WASM API is part of the product.

## Visual Direction

Reuse the visual language from `rulekit-js`:

- dark blue/black CRT background
- scanline/noise texture
- pixel/terminal font, especially `DepartureMono-Regular.woff2`
- layered beveled panels
- blue headers for neutral editor panels
- green result panels for pass
- red result panels for fail
- yellow/orange for missing input
- purple/red for errors
- dim gray for pruned branches
- optional bird/retro sprite assets if they do not distract from the editor

The old visual rule tree prototype is not binding. Keep the core idea of rule expressions as structured visual tokens, but redesign freely for clarity.

## Initial Layout

```text
┌─────────────────────────────┬─────────────────────────────┐
│ Rule Text                   │ Visual Rule Editor           │
│ [Format Compact] [Format ML]│ editable AST representation  │
└─────────────────────────────┴─────────────────────────────┘

┌─────────────────────────────┬─────────────────────────────┐
│ Input JSON                  │ Result + Annotated Trace     │
│ example data buttons        │ read-only colored AST tree   │
└─────────────────────────────┴─────────────────────────────┘
```

For milestone 1, the bottom evaluation row can be stubbed or hidden. The first priority is the text/visual editor sync.

## WASM API Surface

Expose a small browser API from Go. Keep it demo-local.

```ts
parse(source: string): ParseResponse
format(source: string, mode: "compact" | "multiline"): SourceResponse
rewrite(source: string, edit: EditRequest): ParseResponse
deleteNode(source: string, target: NodeRef): ParseResponse
evalRule(source: string, inputJSON: string): EvalResponse
```

Use source text as the unit of exchange initially. Avoid rule handles for the first version. Reparse on each operation; this keeps lifecycle and memory management simple and is fast enough for a demo.

Example DTOs:

```ts
type ParseResponse = {
  ok: boolean
  source?: string
  compact?: string
  multiline?: string
  ast?: AstNode
  tokens?: Token[]
  error?: string
}

type EvalResponse = {
  ok: boolean
  value?: unknown
  error?: string
  missingFields?: string[]
  trace?: TraceNode
}
```

Node references should use parser-produced spans or stable path indexes in the returned AST. Prefer source spans for text selection and rewrite targeting.

## Editor Sync Model

Text edit flow:

1. User edits source text.
2. Debounce parse.
3. If parse succeeds, update visual AST and clear parse error.
4. If parse fails, keep the last valid visual AST and show parse error near the text editor.

Visual edit flow:

1. User selects a visual AST node.
2. User changes an editable field/operator/literal or deletes a safe node.
3. Browser sends `rewrite` or `deleteNode` request to WASM with the current source and target node reference.
4. WASM returns updated source and AST.
5. Text editor updates from returned source.
6. Visual editor updates from returned AST.

Formatting flow:

1. User clicks `Format Compact` or `Format Multiline`.
2. Browser calls `format`.
3. Text editor updates from returned source.
4. Visual editor reparses or uses returned AST if provided.

## Selection Sync

This is central to the demo.

- Clicking a visual node highlights its source range in the text editor.
- Selecting text highlights the smallest AST node whose span contains that selection.
- Delete applies to the selected visual node.
- Format should preserve selection where practical, but this can be a later polish item.

## Visual Editor Scope

Initial editable operations:

- Rewrite a field/path string.
- Rewrite a literal value.
- Rewrite a comparison/operator from a constrained list.
- Rewrite an array literal element value.
- Delete safe nodes.

Initial delete semantics:

- Deleting one side of `and` / `or` collapses the expression to the remaining side.
- Deleting a predicate from a larger same-operator chain preserves the remaining chain.
- Deleting an array element removes only that element.
- Deleting the only element of an array is allowed only if empty arrays are valid for that expression; otherwise disable delete.
- Do not allow deleting just a field/operator/value from a comparison. Require rewrite instead.
- Do not allow deleting function names or partial function syntax in the first version.

No add-node workflow yet.

## Evaluation And Trace UI

After the editor loop works, add evaluation:

- JSON input editor with pass/fail preset buttons.
- Evaluate current valid source against JSON input.
- Show result state:
  - pass
  - fail
  - missing
  - error
- Render a separate read-only annotated visual AST from `Result.Trace`.

Trace tree styling:

- `passed`: green
- `failed`: red
- `missing`: yellow/orange
- `error`: purple/red
- `pruned`: dim gray
- `unknown`: neutral

Trace nodes should use the same visual grammar as the editor tree, but annotated with status, value, missing fields, and pruning state.

## Example Rules

Use examples that show v2 capabilities:

```text
-- exact key with punctuation
request.headers["user-agent"] contains "curl"
```

```text
-- exact top-level dotted key
["destination.ip"] == 192.168.1.1
```

```text
-- multiline boolean tree
(
  dst.port in [3306, 5432, 27017, 6379]
  and src.pod.namespace in ["api", "backend"]
)
or dst.domain in ["registry.k8s.io", "docker.io"]
```

Include comments in examples so source-preserving rewrite is visible.

## Milestones

### Milestone 1: WASM Skeleton

- Create demo directory and Vite/Vue skeleton.
- Copy/adapt font, base CSS, and useful visual assets from `rulekit-js`.
- Build demo-local WASM bridge.
- Implement `parse` and `format` WASM functions.
- Render parse errors and canonical source output.

### Milestone 2: Text + Visual AST Sync

- Render AST as a sideways visual tree.
- Keep text and visual tree synced on valid text edits.
- Keep last valid tree visible on parse errors.
- Implement visual node selection and source-range highlight.

### Milestone 3: Visual Rewrite/Delete

- Implement rewrite for fields, literals, operators, and array elements.
- Implement safe delete semantics.
- Call Go `Rewrite` through WASM and update text from returned source.
- Add compact/multiline format buttons.

### Milestone 4: Evaluation + Annotated Trace

- Add JSON input panel and preset data buttons.
- Add `evalRule` WASM function.
- Render result card.
- Render annotated trace tree as read-only visual AST.

### Milestone 5: Syntax Highlighting

- Add CodeMirror rule syntax highlighting.
- Keep highlighting separate from parser correctness.
- Highlight parse errors and selected AST ranges.

## Open Questions

- Should the first version use CodeMirror, Monaco, or a lightweight textarea?
- Should visual AST nodes be rendered as nested rows, a true sideways graph, or a form-like tree similar to the prototype?
- What exact edit request format should identify a target: source span, AST path, or generated node ID?
- Should JSON input support annotated/typed modes in the first evaluation milestone, or plain JSON only?
- Should demo-local WASM expose raw AST JSON, UI-specific AST JSON, or both?

## Success Criteria

- A user can type a rule and see the visual AST update live.
- A user can edit/delete existing visual nodes and see source update through Go rewrite logic.
- Formatting uses the Go formatter, not a TypeScript formatter.
- Invalid text does not corrupt the visual editor state.
- Evaluation shows a result and a color-coded trace tree.
- The app visibly demonstrates v2 capabilities that v1 did not support well: source spans, formatting, rewrite, bracket paths, and trace explanation.
