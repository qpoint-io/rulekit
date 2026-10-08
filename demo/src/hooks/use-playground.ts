import { useEffect, useMemo, useRef, useState } from "react"

import { narrowestAt, parenthesized, spanKey } from "@/lib/ast"
import {
  deleteRuleNode,
  evalRule,
  flattenAST,
  formatRule,
  loadRulekit,
  onEngineCrash,
  nodeRef,
  parseRule,
  rewriteRule,
  type AstNode,
  type EvalResponse,
  type ParseResponse,
  type SourceResponse,
  type TraceNode,
} from "@/lib/rulekit"
import { SAMPLES, type Sample } from "@/lib/samples"
import { offsetMap } from "@/lib/source-map"

/** State and actions shared by every panel of the playground. */
export interface Playground {
  ready: boolean
  loadError?: string
  /** The last time the engine threw (a crash, not a rule or input problem). */
  crash?: string
  dismissCrash(): void
  source: string
  setSource(next: string): void
  input: string
  setInput(next: string): void
  /** Latest parse of `source`; undefined until the engine loads. */
  parsed?: ParseResponse
  /** The latest parse that succeeded; views keep showing it while the text is broken. */
  shown?: ParseResponse
  /** True when `shown` is older than the current text (which doesn't parse). */
  stale: boolean
  /** Evaluation of `source` against `input`; undefined while the rule doesn't parse. */
  result?: EvalResponse
  nodes: AstNode[]
  byId: Map<string, AstNode>
  /** Keyed by `spanKey`; links trace entries back to AST nodes. */
  bySpan: Map<string, AstNode>
  traceBySpan: Map<string, TraceNode>
  /** Ids of and/or nodes the source wraps in their own parentheses. */
  grouped: Set<string>
  selected?: AstNode
  hovered?: AstNode
  select(node?: AstNode): void
  /** Select whatever node the rule editor's caret is in, kept in step as the text changes. */
  selectAt(start: number, end: number): void
  /** Select `node`, or clear the selection if it's already selected. */
  toggle(node: AstNode): void
  hover(node?: AstNode): void
  /** Last failed format/rewrite/delete; cleared by the next source change. */
  editError?: string
  format(mode: "compact" | "multiline"): Promise<void>
  rewrite(
    node: AstNode,
    replacement: string,
    kind: "operator" | "node"
  ): Promise<void>
  remove(node: AstNode): Promise<void>
  /** Replace the whole rule as one undo step (structural edits from a view). */
  /** Replace the whole rule from a view; `action` describes it for the log. */
  replace(next: string, action: string): void
  /** Every change this session, oldest first, for reproducing a state. */
  log: LogEntry[]
  /** The log as plain text, ready to paste into a bug report or chat. */
  logText(): string
  clearLog(): void
  /** The example last loaded (the first one until another is picked). */
  example: Sample
  /** Restore the rule and input of `example`. */
  reset(): void
  /** Load a sample's rule and input together; the rule change is one undo step. */
  load(sample: Sample): void
  /** Undo/redo source changes, whether typed or made through a view. */
  undo(): void
  redo(): void
  canUndo: boolean
  canRedo: boolean
  /** A URL that reopens the playground with this rule and input. */
  shareUrl(): string
}

/** One change to the rule or input, with the state it left behind. */
export type LogEntry = {
  at: string
  /** Where the change came from. */
  source: "start" | "editor" | "input" | "builder" | "view" | "app"
  action: string
  /** The rule and/or input after this change (only the parts it changed). */
  rule?: string
  input?: string
}

const LOG_LIMIT = 300

const STORAGE_KEY = "rulekit-playground"
const EXAMPLE_KEY = "rulekit-playground-example"

const encode = (text: string) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(text)))
const decode = (b64: string) =>
  new TextDecoder().decode(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)))

/** Rule and input to start from: a shared link, then the last session, then the example. */
function initialState(): { source: string; input: string } {
  const fallback = {
    source: SAMPLES[0].rule,
    input: SAMPLES[0].json,
  }
  try {
    const hash = new URLSearchParams(location.hash.slice(1))
    const r = hash.get("rule")
    if (r !== null)
      return {
        source: decode(r),
        input: decode(hash.get("input") ?? encode(fallback.input)),
      }
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null")
    if (typeof saved?.source === "string" && typeof saved?.input === "string")
      return saved
  } catch {
    // A malformed link or storage entry: start fresh.
  }
  return fallback
}

/** Typing within this long of the previous change merges into one undo step. */
const MERGE_MS = 800

export function usePlayground(): Playground {
  const [ready, setReady] = useState(false)
  const [loadError, setLoadError] = useState<string>()
  const [initial] = useState(initialState)
  const [exampleId, setExampleId] = useState(
    () => localStorage.getItem(EXAMPLE_KEY) ?? SAMPLES[0].id
  )
  const [source, setSourceState] = useState(initial.source)
  const [input, setInputState] = useState(initial.input)
  const [log, setLog] = useState<LogEntry[]>(() => [
    {
      at: clock(),
      source: "start",
      action: "session started",
      rule: initial.source,
      input: initial.input,
    },
  ])
  /**
   * Append a log entry. Typing (in the rule editor or the input) arrives a
   * keystroke at a time, so a run of the same `merge` action updates the last
   * entry instead of adding one per key.
   */
  const record = (entry: Omit<LogEntry, "at">, merge = false) =>
    setLog((prev) => {
      const last = prev.at(-1)
      // Actions quote rule text; keep each on one line.
      const next = {
        ...entry,
        action: entry.action.replace(/\s+/g, " "),
        at: clock(),
      }
      if (
        merge &&
        last &&
        last.source === entry.source &&
        last.action === entry.action
      ) {
        return [...prev.slice(0, -1), next]
      }
      return [...prev, next].slice(-LOG_LIMIT)
    })
  const setInput = (next: string) => {
    setInputState(next)
    record(
      { source: "input", action: "edited the input JSON", input: next },
      true
    )
  }
  const history = useRef({
    past: [] as string[],
    future: [] as string[],
    lastTyped: 0,
  })
  const [depth, setDepth] = useState({ undo: 0, redo: 0 })

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ source, input }))
  }, [source, input])
  const [parsed, setParsed] = useState<ParseResponse>()
  const [shown, setShown] = useState<ParseResponse>()
  const [result, setResult] = useState<EvalResponse>()
  const [selectedId, setSelectedId] = useState<string>()
  /**
   * Caret byte range when the selection comes from the rule editor. Node ids
   * are tree positions and shift as the rule is edited (typing `or …` adds a
   * level), so an editor selection is re-resolved from the caret on every
   * parse instead of trusting an id that may now name another node.
   */
  const [caret, setCaret] = useState<{ start: number; end: number }>()
  const [hoveredId, setHoveredId] = useState<string>()
  const [editError, setEditError] = useState<string>()

  const [crash, setCrash] = useState<string>()
  useEffect(
    () =>
      onEngineCrash((message) => {
        setCrash(message)
        record({ source: "app", action: `rule engine crashed: ${message}` })
      }),
    // Subscribe once; `record` only appends to the log.
    []
  )

  useEffect(() => {
    loadRulekit().then(
      () => setReady(true),
      (err) => setLoadError(err instanceof Error ? err.message : String(err))
    )
  }, [])

  useEffect(() => {
    if (!ready) return
    let live = true
    void (async () => {
      const nextParsed = await parseRule(source)
      const nextResult = nextParsed.ok
        ? await evalRule(source, input)
        : undefined
      if (!live) return
      setParsed(nextParsed)
      if (nextParsed.ok) setShown(nextParsed)
      setResult(nextResult)
    })()
    return () => {
      live = false
    }
  }, [ready, source, input])

  const nodes = useMemo(() => flattenAST(shown?.ast), [shown])
  const byId = useMemo(
    () => new Map(nodes.map((node) => [node.id, node])),
    [nodes]
  )
  const bySpan = useMemo(() => {
    const map = new Map<string, AstNode>()
    // First wins, so a span shared by parent and child resolves to the outer node.
    for (const node of nodes)
      if (!map.has(spanKey(node.span))) map.set(spanKey(node.span), node)
    return map
  }, [nodes])
  const traceBySpan = useMemo(() => {
    const map = new Map<string, TraceNode>()
    const walk = (trace: TraceNode) => {
      if (trace.span && !map.has(spanKey(trace.span)))
        map.set(spanKey(trace.span), trace)
      trace.children?.forEach(walk)
    }
    if (result?.trace) walk(result.trace)
    return map
  }, [result])

  const grouped = useMemo(
    () => parenthesized(nodes, shown?.tokens ?? []),
    [shown, nodes]
  )

  const fresh = !!parsed?.ok
  const selected = caret
    ? fresh
      ? nodeAtCaret(nodes, source, caret)
      : undefined
    : selectedId
      ? byId.get(selectedId)
      : undefined
  const hovered = hoveredId ? byId.get(hoveredId) : undefined

  /** Change the source, recording an undo step unless it continues a burst of typing. */
  function commit(next: string, typed: boolean) {
    if (next === source) return
    // Hover ids are tree positions too; after an edit they may name another node.
    setHoveredId(undefined)
    const h = history.current
    const now = Date.now()
    if (!(typed && now - h.lastTyped < MERGE_MS)) h.past.push(source)
    h.lastTyped = typed ? now : 0
    h.future = []
    setSourceState(next)
    setEditError(undefined)
    setDepth({
      undo: history.current.past.length,
      redo: history.current.future.length,
    })
  }
  const setSource = (next: string) => commit(next, false)

  function applySource(res: SourceResponse | ParseResponse, fallback: string) {
    if (!res.ok || res.source === undefined) {
      setEditError(res.error || fallback)
      return false
    }
    setSource(res.source)
    return true
  }

  const step = (from: string[], to: string[], action: string) => {
    const prev = from.pop()
    if (prev === undefined) return
    to.push(source)
    record({ source: "app", action, rule: prev })
    history.current.lastTyped = 0
    setSourceState(prev)
    setEditError(undefined)
    setDepth({
      undo: history.current.past.length,
      redo: history.current.future.length,
    })
  }

  return {
    ready,
    loadError,
    crash,
    dismissCrash: () => setCrash(undefined),
    source,
    setSource: (next: string) => {
      commit(next, true)
      record(
        { source: "editor", action: "edited the rule text", rule: next },
        true
      )
    },
    input,
    setInput,
    parsed,
    shown,
    stale: !!parsed && !parsed.ok && !!shown,
    result,
    nodes,
    byId,
    bySpan,
    traceBySpan,
    grouped,
    selected,
    hovered,
    select: (node?: AstNode) => {
      setCaret(undefined)
      setSelectedId(node?.id)
    },
    selectAt: (start: number, end: number) => setCaret({ start, end }),
    toggle: (node: AstNode) => {
      setCaret(undefined)
      setSelectedId(selected?.id === node.id ? undefined : node.id)
    },
    hover: (node?: AstNode) => setHoveredId(node?.id),
    editError,
    async format(mode: "compact" | "multiline") {
      const res = await formatRule(source, mode)
      if (applySource(res, "Couldn’t format the rule."))
        record({
          source: "editor",
          action: `format (${mode})`,
          rule: res.source,
        })
    },
    async rewrite(
      node: AstNode,
      replacement: string,
      kind: "operator" | "node"
    ) {
      const res = await rewriteRule(source, {
        target: nodeRef(node),
        replacement,
        kind,
        mode: "source",
      })
      if (applySource(res, "Couldn’t apply the change."))
        record({
          source: "view",
          action: `set ${kind} "${node.text}" to "${replacement}"`,
          rule: res.source,
        })
    },
    async remove(node: AstNode) {
      const res = await deleteRuleNode(source, nodeRef(node))
      if (applySource(res, "Couldn’t delete the node.")) {
        record({
          source: "view",
          action: `delete "${node.text}"`,
          rule: res.source,
        })
        setSelectedId(undefined)
        setCaret(undefined)
        setHoveredId(undefined)
      }
    },
    replace(next: string, action: string) {
      setSource(next)
      record({ source: "builder", action, rule: next })
    },
    log,
    logText: () => formatLog(log, source, input),
    clearLog: () =>
      setLog([
        {
          at: clock(),
          source: "start",
          action: "log cleared",
          rule: source,
          input,
        },
      ]),
    undo: () => step(history.current.past, history.current.future, "undo"),
    redo: () => step(history.current.future, history.current.past, "redo"),
    canUndo: depth.undo > 0,
    canRedo: depth.redo > 0,
    shareUrl: () =>
      `${location.origin}${location.pathname}#${new URLSearchParams({ rule: encode(source), input: encode(input) })}`,
    example: SAMPLES.find((x) => x.id === exampleId) ?? SAMPLES[0],
    load(sample: Sample) {
      setExampleId(sample.id)
      localStorage.setItem(EXAMPLE_KEY, sample.id)
      setSource(sample.rule)
      setInputState(sample.json)
      setSelectedId(undefined)
      setCaret(undefined)
      record({
        source: "app",
        action: `load example "${sample.label}"`,
        rule: sample.rule,
        input: sample.json,
      })
    },
    reset() {
      const sample = SAMPLES.find((x) => x.id === exampleId) ?? SAMPLES[0]
      setSource(sample.rule)
      setInputState(sample.json)
      setSelectedId(undefined)
      setCaret(undefined)
      record({
        source: "app",
        action: `reset to example "${sample.label}"`,
        rule: sample.rule,
        input: sample.json,
      })
    },
  }
}

const clock = () => new Date().toTimeString().slice(0, 8)

const fence = (lang: string, text: string) => `\`\`\`${lang}\n${text}\n\`\`\``

/** The log as Markdown: each step with the rule/input it produced, then the final state. */
function formatLog(log: LogEntry[], source: string, input: string): string {
  const lines = [`# rulekit playground log (${log.length} entries)`, ""]
  log.forEach((entry, i) => {
    lines.push(`${i + 1}. [${entry.at}] ${entry.source}: ${entry.action}`)
    if (entry.rule !== undefined) lines.push(fence("rulekit", entry.rule))
    if (entry.input !== undefined) lines.push(fence("json", entry.input))
  })
  lines.push(
    "",
    "## Final state",
    fence("rulekit", source),
    fence("json", input)
  )
  return lines.join("\n")
}

/**
 * The node under the rule editor's caret. A bare caret sitting in whitespace
 * (just after `or tls.sni `, say) belongs to the token before it, so the
 * selection and its link stay put until something else is typed.
 */
function nodeAtCaret(
  nodes: AstNode[],
  source: string,
  caret: { start: number; end: number }
) {
  const hit = narrowestAt(nodes, caret.start, caret.end)
  if (hit || caret.start !== caret.end) return hit
  const map = offsetMap(source)
  let at = map.toUtf16(caret.start)
  while (at > 0 && /\s/.test(source[at - 1])) at--
  const byte = map.toByte(at)
  return byte === caret.start ? undefined : narrowestAt(nodes, byte, byte)
}
