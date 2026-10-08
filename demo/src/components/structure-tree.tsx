import { useLayoutEffect, useRef, useState, type ReactNode } from "react"

import { spanKey } from "@/lib/ast"
import type { AstNode, TraceNode } from "@/lib/rulekit"
import { cn } from "@/lib/utils"

import { STATUS } from "./status"

const KIND_LABEL: Record<string, string> = {
  binary: "compare",
  unary: "negate",
  path: "field",
  literal: "value",
  array: "list",
}

export function kindLabel(node: AstNode) {
  if (
    node.kind === "binary" &&
    (node.operator === "and" || node.operator === "or")
  )
    return "logic"
  return KIND_LABEL[node.kind] ?? node.kind
}

/** The node's own text (operator, field, or value) and the editor color it gets there. */
export function nodeLabel(node: AstNode): { text: string; className: string } {
  if (node.kind === "path")
    return { text: node.path || node.text, className: "text-tok-id" }
  if (node.kind === "array") {
    return {
      text: `${node.children?.length ?? 0} items`,
      className: "text-muted-foreground",
    }
  }
  if (node.kind === "literal") {
    const raw = node.raw || node.text
    const className = /^["']/.test(raw)
      ? "text-tok-str"
      : /^(true|false)$/.test(raw)
        ? "text-tok-const"
        : raw.startsWith("/")
          ? "text-tok-regex"
          : "text-tok-num"
    return { text: raw, className }
  }
  const text = node.raw || node.operator || node.kind
  return {
    text,
    className: /^[a-z]/i.test(text) ? "font-medium text-tok-kw" : "text-tok-op",
  }
}

/** Why a step came out the way it did: missing fields, type diagnostics, errors. */
function traceNotes(trace?: TraceNode): string[] {
  if (!trace) return []
  return [
    ...(trace.missingFields?.length
      ? [`missing ${trace.missingFields.join(", ")}`]
      : []),
    ...(trace.diagnostics?.map((d) => d.Message) ?? []),
    ...(trace.error ? [trace.error] : []),
  ]
}

/** What the evaluator produced at this node, when that adds something beyond the status icon. */
function shownValue(node: AstNode, trace?: TraceNode): string | undefined {
  if (!trace) return undefined
  if (trace.status === "pruned") return "pruned"
  // Literals evaluate to themselves; repeating them is noise.
  if (
    trace.value === undefined ||
    node.kind === "literal" ||
    node.kind === "array"
  )
    return undefined
  return JSON.stringify(trace.value)
}

const INDENT = 20

/** Indent guides cycle through these so each nesting level has its own color. */
const GUIDE_COLORS = [
  "bg-tok-kw/70",
  "bg-tok-id/70",
  "bg-tok-str/70",
  "bg-tok-num/70",
  "bg-tok-regex/70",
  "bg-tok-const/70",
]

export type StructureProps = {
  /** Inside a pruned branch: the evaluator never reached this node. */
  dimmed?: boolean
  node: AstNode
  selectedId?: string
  hoveredId?: string
  traceBySpan: Map<string, TraceNode>
  /** Nodes the source wraps in parentheses; they stay separate groups. */
  grouped: Set<string>
  onSelect(node: AstNode): void
  onHover(node?: AstNode): void
}

export function rowHandlers(
  node: AstNode,
  { onSelect, onHover }: Pick<StructureProps, "onSelect" | "onHover">
) {
  return {
    onClick: () => onSelect(node),
    onMouseEnter: () => onHover(node),
    onMouseLeave: () => onHover(),
    onFocus: () => onHover(node),
    onBlur: () => onHover(),
  }
}

export function StructureTree({
  node,
  depth = 0,
  ...props
}: StructureProps & { depth?: number }) {
  const { selectedId, hoveredId, traceBySpan } = props
  const trace = traceBySpan.get(spanKey(node.span))
  const status = trace ? STATUS[trace.status] : undefined
  const selected = node.id === selectedId
  // A chain like `a and b and c` parses as nested ands; list it as one.
  const children = isLogic(node)
    ? logicChildren(node, props.grouped)
    : (node.children ?? [])
  const label = nodeLabel(node)
  const value = shownValue(node, trace)
  const notes = traceNotes(trace)

  return (
    <li
      role="treeitem"
      aria-selected={selected}
      aria-expanded={node.children?.length ? true : undefined}
    >
      <button
        type="button"
        data-node-id={node.id}
        className={cn(
          "relative flex min-h-7 w-full items-start gap-2 rounded-md py-1 pr-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/60",
          node.id === hoveredId && !selected && "bg-accent/70",
          selected && "bg-selection/10",
          (props.dimmed || trace?.status === "pruned") && "opacity-55"
        )}
        style={{ paddingLeft: `${depth * INDENT + 8}px` }}
        {...rowHandlers(node, props)}
      >
        {selected && (
          <span
            aria-hidden
            className="absolute inset-y-1 left-0 w-0.5 rounded-full bg-selection"
          />
        )}
        {Array.from({ length: depth }, (_, i) => (
          <span
            aria-hidden
            key={i}
            className={cn(
              "absolute inset-y-0 w-px",
              GUIDE_COLORS[i % GUIDE_COLORS.length]
            )}
            style={{ left: `${i * INDENT + 14}px` }}
          />
        ))}
        {status ? (
          <status.icon
            aria-label={status.label}
            className={cn("mt-[3px] size-3.5 shrink-0", status.tone)}
          />
        ) : (
          <span aria-hidden className="size-3.5 shrink-0" />
        )}
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex items-baseline gap-2">
            <span
              className={cn("truncate font-mono text-[13px]", label.className)}
              title={node.text}
            >
              {label.text}
            </span>
            <span className="shrink-0 text-xs text-muted-foreground">
              {kindLabel(node)}
            </span>
            {value && (
              <span
                className="ml-auto truncate pl-2 font-mono text-xs text-muted-foreground"
                title={value}
              >
                {value}
              </span>
            )}
          </span>
          {notes.map((note) => (
            <span key={note} className={cn("text-xs", status?.tone)}>
              {note}
            </span>
          ))}
        </span>
      </button>
      {children.length ? (
        <ul role="group">
          {children.map((child) => (
            <StructureTree
              key={child.id}
              node={child}
              depth={depth + 1}
              {...props}
              dimmed={props.dimmed || trace?.status === "pruned"}
            />
          ))}
        </ul>
      ) : null}
    </li>
  )
}

export function tooltipFor(node: AstNode, trace?: TraceNode) {
  const status = trace ? STATUS[trace.status] : undefined
  const value = shownValue(node, trace)
  return [
    `${node.text} (${kindLabel(node)})`,
    ...(status
      ? [`${status.label}${value && value !== "pruned" ? `: ${value}` : ""}`]
      : []),
    ...traceNotes(trace),
  ].join("\n")
}

/** and / or / not: the nodes that branch in the graph and nest in blocks. */
export function isLogic(node: AstNode) {
  return (
    node.kind === "unary" ||
    (node.kind === "binary" &&
      (node.operator === "and" || node.operator === "or"))
  )
}

/** and/or parse left-nested; a chain of one operator reads better as one fan-out. */
export function logicChildren(node: AstNode, grouped: Set<string>): AstNode[] {
  const out: AstNode[] = []
  const gather = (n: AstNode) => {
    for (const child of n.children ?? []) {
      // Same-operator links merge, unless the source wrapped them in parentheses.
      if (
        n.kind === "binary" &&
        child.kind === "binary" &&
        child.operator === n.operator &&
        !grouped.has(child.id)
      )
        gather(child)
      else out.push(child)
    }
  }
  gather(node)
  return out
}

const PUN = "font-mono text-[13px] text-tok-pun"

/**
 * A node as selectable chips in source order. Comparisons get a status-colored
 * underline; nested logic of a different operator gets parentheses.
 */
function InlineNode({ node, ...props }: StructureProps) {
  const { selectedId, hoveredId, traceBySpan } = props
  const trace = traceBySpan.get(spanKey(node.span))
  const chip = (text: string, className: string, extra?: ReactNode) => (
    <button
      type="button"
      data-node-id={node.id}
      title={tooltipFor(node, trace)}
      className={cn(
        "rounded px-1 text-left font-mono text-[13px] leading-5 [overflow-wrap:anywhere] outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/60",
        className,
        node.id === hoveredId && node.id !== selectedId && "bg-accent",
        node.id === selectedId && "bg-selection/15 ring-1 ring-selection/60"
      )}
      {...rowHandlers(node, props)}
    >
      {text}
      {extra}
    </button>
  )
  const label = nodeLabel(node)
  const children = node.children ?? []
  const grouped = (child: AstNode) =>
    isLogic(child) &&
    child.kind === "binary" &&
    child.operator !== node.operator ? (
      <span className="inline-flex flex-wrap items-center">
        <span className={PUN}>(</span>
        <InlineNode node={child} {...props} />
        <span className={PUN}>)</span>
      </span>
    ) : (
      <InlineNode node={child} {...props} />
    )

  if (node.kind === "unary" && children.length === 1) {
    return (
      <span className="inline-flex flex-wrap items-center">
        {chip(label.text, label.className)}
        <span className={PUN}>(</span>
        <InlineNode node={children[0]} {...props} />
        <span className={PUN}>)</span>
      </span>
    )
  }
  if (node.kind === "binary" && children.length === 2) {
    // Operator and right side form one unit, so a wrap puts the operator at
    // the start of the next line rather than dangling at the end of this one.
    const body = (
      <>
        {grouped(children[0])}
        <span className="inline-flex min-w-0 items-start">
          {chip(label.text, cn(label.className, "shrink-0 whitespace-nowrap"))}
          {grouped(children[1])}
        </span>
      </>
    )
    return body
  }
  if (node.kind === "array") {
    return (
      <span className="inline-flex flex-wrap items-center">
        {chip("[", "text-tok-pun")}
        {children.map((child, i) => (
          <span key={child.id} className="inline-flex items-center">
            <InlineNode node={child} {...props} />
            {i < children.length - 1 && (
              <span className={cn(PUN, "mr-0.5 -ml-0.5")}>,</span>
            )}
          </span>
        ))}
        <span className={PUN}>]</span>
      </span>
    )
  }
  const value = node.kind === "path" ? shownValue(node, trace) : undefined
  return chip(
    label.text,
    label.className,
    value && (
      <span className="ml-1 text-[11px] text-muted-foreground">{value}</span>
    )
  )
}

/** One comparison as a card: status icon plus its chips. */
function ComparisonBox({
  node,
  className,
  ...props
}: StructureProps & { className?: string }) {
  const trace = props.traceBySpan.get(spanKey(node.span))
  const status = trace ? STATUS[trace.status] : undefined
  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-lg bg-card px-2.5 py-1 ring-1 ring-border",
        (props.dimmed || trace?.status === "pruned") && "opacity-55",
        className
      )}
    >
      {status && (
        <status.icon
          aria-label={status.label}
          className={cn("mt-[3px] size-3.5 shrink-0", status.tone)}
        />
      )}
      <span className="flex flex-wrap items-center gap-0.5">
        <InlineNode node={node} {...props} />
      </span>
    </div>
  )
}

const LOGIC_PHRASE: Record<string, string> = {
  and: "&&",
  or: "||",
}

/** The logic node's own selectable label, used by graph and blocks. */
function LogicPill({
  node,
  phrase,
  ...props
}: StructureProps & { phrase?: boolean }) {
  const { selectedId, hoveredId, traceBySpan } = props
  const trace = traceBySpan.get(spanKey(node.span))
  const status = trace ? STATUS[trace.status] : undefined
  const selected = node.id === selectedId
  const label = nodeLabel(node)
  return (
    <button
      type="button"
      data-node-id={node.id}
      title={tooltipFor(node, trace)}
      className={cn(
        "flex h-7 w-fit shrink-0 items-center gap-1.5 rounded-full bg-card px-2.5 whitespace-nowrap ring-1 ring-border outline-none focus-visible:ring-2 focus-visible:ring-ring/60",
        node.id === hoveredId && !selected && "bg-accent",
        selected && "bg-selection/15 ring-2 ring-selection",
        (props.dimmed || trace?.status === "pruned") && "opacity-55"
      )}
      {...rowHandlers(node, props)}
    >
      {status && (
        <status.icon
          aria-label={status.label}
          className={cn("size-3.5 shrink-0", status.tone)}
        />
      )}
      <span className={cn("font-mono text-[13px]", label.className)}>
        {label.text}
      </span>
      {phrase && LOGIC_PHRASE[label.text] && (
        <span className="font-mono text-xs text-muted-foreground">
          {LOGIC_PHRASE[label.text]}
        </span>
      )}
    </button>
  )
}

/**
 * The rule drawn as a left-to-right tree. Only logic (and, or, not) branches;
 * each comparison is one box whose parts are still individually selectable.
 * Siblings stack vertically, so the width is bounded by nesting depth.
 */
export function StructureGraph({ node, ...props }: StructureProps) {
  if (!isLogic(node))
    return <ComparisonBox node={node} className="max-w-80" {...props} />
  const children = logicChildren(node, props.grouped)
  const traceStatus = props.traceBySpan.get(spanKey(node.span))?.status
  // Every node's first line is centered 14px below its top (pills are h-7,
  // boxes put a 20px line under py-1). Child rows add 6px of padding, which
  // -my-1.5 cancels for the first row, so the stem (14px) meets the first
  // child's stub (20px into its row) exactly; children of any height align.
  return (
    <div className="flex items-start">
      <div className="flex shrink-0 items-start">
        <LogicPill node={node} {...props} />
        <span aria-hidden className="mt-[14px] h-px w-6 shrink-0 bg-border" />
      </div>
      <div className="-my-1.5 flex flex-col">
        {children.map((child, i) => (
          <div key={child.id} className="relative flex items-start py-1.5 pl-6">
            {i > 0 && (
              <span
                aria-hidden
                className="absolute top-0 left-0 h-[20px] w-px bg-border"
              />
            )}
            {i < children.length - 1 && (
              <span
                aria-hidden
                className="absolute top-[20px] bottom-0 left-0 w-px bg-border"
              />
            )}
            <span
              aria-hidden
              className="absolute top-[20px] left-0 h-px w-6 bg-border"
            />
            <StructureGraph
              node={child}
              {...props}
              dimmed={props.dimmed || traceStatus === "pruned"}
            />
          </div>
        ))}
      </div>
    </div>
  )
}

/**
 * Logic as nested blocks, each
 * with a rail in its result color; comparisons sit inside as cards.
 */
export function StructureBlocks({ node, ...props }: StructureProps) {
  if (!isLogic(node)) return <ComparisonBox node={node} {...props} />
  const trace = props.traceBySpan.get(spanKey(node.span))
  const status = trace ? STATUS[trace.status] : undefined
  return (
    <div
      className={cn(
        "flex flex-col gap-2 rounded-lg border border-l-[3px] bg-foreground/[0.025] p-2",
        status?.rail ?? "border-l-border",
        trace?.status === "pruned" && "opacity-70"
      )}
    >
      <LogicPill node={node} phrase {...props} />
      <div className="flex flex-col gap-2 pl-3">
        {logicChildren(node, props.grouped).map((child) => (
          <StructureBlocks key={child.id} node={child} {...props} />
        ))}
      </div>
    </div>
  )
}

const MIN_SCALE = 0.75

/**
 * Shrinks wide content to the container's width, down to MIN_SCALE; past
 * that the container scrolls horizontally.
 */
export function FitWidth({ children }: { children: ReactNode }) {
  const outer = useRef<HTMLDivElement>(null)
  const inner = useRef<HTMLDivElement>(null)
  const [fit, setFit] = useState({ scale: 1, width: 0, height: 0 })

  useLayoutEffect(() => {
    const measure = () => {
      if (!outer.current || !inner.current) return
      const width = inner.current.offsetWidth
      const height = inner.current.offsetHeight
      const scale = Math.max(
        MIN_SCALE,
        Math.min(1, outer.current.clientWidth / width)
      )
      setFit((prev) =>
        prev.scale === scale && prev.width === width && prev.height === height
          ? prev
          : { scale, width, height }
      )
    }
    // Measure before first paint; waiting for the observer would flash the unscaled graph.
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(outer.current!)
    observer.observe(inner.current!)
    return () => observer.disconnect()
  }, [])

  return (
    <div ref={outer} className="min-h-0 flex-1 overflow-x-auto overflow-y-auto">
      <div
        className="mx-auto"
        style={{
          width: fit.width * fit.scale,
          height: fit.height * fit.scale,
          visibility: fit.width ? undefined : "hidden",
        }}
      >
        <div
          ref={inner}
          className="w-max origin-top-left py-2"
          style={{ transform: `scale(${fit.scale})` }}
        >
          {children}
        </div>
      </div>
    </div>
  )
}
