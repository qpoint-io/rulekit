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

export type TraceNode = {
  kind?: string
  expr?: string
  value?: unknown
  error?: string
  missingFields?: string[]
  diagnostics?: Diagnostic[]
  status?: string
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
  status?: string
  error?: string
  missingFields?: string[]
  trace?: TraceNode
  ast?: AstNode
}

export type NodeRef = Pick<AstNode, 'id'> & { start: number; end: number }

export type RewriteRequest = {
  target: NodeRef
  replacement: string
  kind?: 'node' | 'operator'
  mode?: 'source' | 'compact' | 'multiline'
}

type WasmAPI = {
  parse(source: string): string
  format(source: string, mode: string): string
  rewrite(source: string, edit: string): string
  deleteNode(source: string, target: string): string
  evalRule(source: string, inputJSON: string): string
}

declare global {
  interface Window {
    Go: new () => { importObject: WebAssembly.Imports; run(instance: WebAssembly.Instance): Promise<void> }
    rulekitWasm?: WasmAPI
  }
}

let loading: Promise<WasmAPI> | null = null

export function loadRulekit(): Promise<WasmAPI> {
  if (window.rulekitWasm) return Promise.resolve(window.rulekitWasm)
  if (loading) return loading

  loading = new Promise((resolve, reject) => {
    const start = async () => {
      try {
        const go = new window.Go()
        const wasm = await WebAssembly.instantiateStreaming(fetch('/rulekit.wasm'), go.importObject)
        void go.run(wasm.instance)
        const wait = () => {
          if (window.rulekitWasm) {
            resolve(window.rulekitWasm)
            return
          }
          window.setTimeout(wait, 10)
        }
        wait()
      } catch (err) {
        reject(err)
      }
    }

    if (window.Go) {
      void start()
      return
    }

    const script = document.createElement('script')
    script.src = '/wasm_exec.js'
    script.onload = () => void start()
    script.onerror = () => reject(new Error('failed to load wasm_exec.js'))
    document.head.appendChild(script)
  })

  return loading
}

function parseJSON<T>(raw: string): T {
  return JSON.parse(raw) as T
}

export async function parseRule(source: string): Promise<ParseResponse> {
  const api = await loadRulekit()
  return parseJSON(api.parse(source))
}

export async function formatRule(source: string, mode: 'compact' | 'multiline'): Promise<SourceResponse> {
  const api = await loadRulekit()
  return parseJSON(api.format(source, mode))
}

export async function rewriteRule(source: string, edit: RewriteRequest): Promise<ParseResponse> {
  const api = await loadRulekit()
  return parseJSON(api.rewrite(source, JSON.stringify(edit)))
}

export async function deleteRuleNode(source: string, target: NodeRef): Promise<ParseResponse> {
  const api = await loadRulekit()
  return parseJSON(api.deleteNode(source, JSON.stringify(target)))
}

export async function evalRule(source: string, inputJSON: string): Promise<EvalResponse> {
  const api = await loadRulekit()
  return parseJSON(api.evalRule(source, inputJSON))
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
