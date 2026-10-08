import init, * as wasm from "../wasm/rulekit.js"

export type Span = {
  start: number
  end: number
  startLine: number
  startColumn: number
  endLine: number
  endColumn: number
}

export type AstNode = {
  id: string
  kind: string
  text: string
  operator?: string
  negated?: boolean
  raw?: string
  path?: string
  span: Span
  children?: AstNode[]
}

export type Token = {
  kind: string
  role: string
  raw: string
  span: Span
}

export type Diagnostic = {
  Code: string
  Message: string
  LeftType: string
  Operator: string
  RightType: string
}

export type Status =
  "passed" | "failed" | "missing" | "error" | "pruned" | "unknown"

export type TraceNode = {
  kind?: string
  expr?: string
  value?: unknown
  error?: string
  missingFields?: string[]
  diagnostics?: Diagnostic[]
  status: Status
  active?: boolean
  pruned?: boolean
  span?: Span
  children?: TraceNode[]
}

export type ParseResponse = {
  ok: boolean
  source?: string
  compact?: string
  multiline?: string
  ast?: AstNode
  tokens?: Token[]
  error?: string
}

export type SourceResponse = {
  ok: boolean
  source?: string
  ast?: AstNode
  error?: string
}

export type EvalResponse = {
  ok: boolean
  value?: unknown
  status?: Exclude<Status, "pruned">
  error?: string
  missingFields?: string[]
  trace?: TraceNode
  ast?: AstNode
}

export type NodeRef = Pick<AstNode, "id"> & { start: number; end: number }

export type RewriteRequest = {
  target: NodeRef
  replacement: string
  kind?: "node" | "operator"
  mode?: "source" | "compact" | "multiline"
}

type WasmAPI = typeof wasm

let loading: Promise<WasmAPI> | null = null

export function loadRulekit(): Promise<WasmAPI> {
  loading ??= init().then(() => wasm)
  return loading
}

/** Called with the message whenever the engine throws instead of answering. */
const crashListeners = new Set<(message: string) => void>()

/** Subscribe to engine crashes (a Rust panic surfaces as a thrown wasm trap). */
export function onEngineCrash(listener: (message: string) => void): () => void {
  crashListeners.add(listener)
  return () => crashListeners.delete(listener)
}

/**
 * Run one bridge call. Rulekit reports parse and evaluation problems inside
 * its JSON response; anything thrown is a crash, so it's reported to the
 * crash listeners and turned into a failed response the callers already handle.
 */
function call<T extends { ok: boolean; error?: string }>(run: () => string): T {
  try {
    return JSON.parse(run()) as T
  } catch (err) {
    const message =
      err instanceof Error ? `${err.name}: ${err.message}` : String(err)
    crashListeners.forEach((listener) => listener(message))
    return { ok: false, error: `The rule engine crashed: ${message}` } as T
  }
}

export async function parseRule(source: string): Promise<ParseResponse> {
  const api = await loadRulekit()
  return call(() => api.parse(source))
}

export async function formatRule(
  source: string,
  mode: "compact" | "multiline"
): Promise<SourceResponse> {
  const api = await loadRulekit()
  return call(() => api.format(source, mode))
}

export async function rewriteRule(
  source: string,
  edit: RewriteRequest
): Promise<ParseResponse> {
  const api = await loadRulekit()
  return call(() => api.rewrite(source, JSON.stringify(edit)))
}

export async function deleteRuleNode(
  source: string,
  target: NodeRef
): Promise<ParseResponse> {
  const api = await loadRulekit()
  return call(() => api.deleteNode(source, JSON.stringify(target)))
}

export async function evalRule(
  source: string,
  inputJSON: string
): Promise<EvalResponse> {
  const api = await loadRulekit()
  return call(() => api.evalRule(source, inputJSON))
}

export function flattenAST(root?: AstNode): AstNode[] {
  if (!root) return []
  const out: AstNode[] = []
  const walk = (node: AstNode) => {
    out.push(node)
    node.children?.forEach(walk)
  }
  walk(root)
  return out
}

export function nodeRef(node: AstNode): NodeRef {
  return { id: node.id, start: node.span.start, end: node.span.end }
}
