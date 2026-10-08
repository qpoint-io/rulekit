import type { AstNode, Span, Token } from "./rulekit"

export const spanKey = (span: Pick<Span, "start" | "end">) =>
  `${span.start}:${span.end}`

/** The narrowest node whose byte span covers [start, end). */
export function narrowestAt(
  nodes: AstNode[],
  start: number,
  end: number
): AstNode | undefined {
  let best: AstNode | undefined
  for (const node of nodes) {
    if (node.span.start > start || node.span.end < end) continue
    if (
      !best ||
      node.span.end - node.span.start <= best.span.end - best.span.start
    )
      best = node
  }
  return best
}

export type EditKind = "operator" | "path" | "literal"

export function editKind(node: AstNode): EditKind | undefined {
  if (node.kind === "binary") return "operator"
  if (node.kind === "path" || node.kind === "literal") return node.kind
  return undefined
}

/** Value the inspector starts from: the operator id, field path, or literal source. */
export function editDraft(node: AstNode): string {
  if (node.kind === "binary")
    return `${node.negated ? "not_" : ""}${node.operator ?? ""}`
  if (node.kind === "path") return node.path || node.text
  return node.raw || node.text
}

export const OPERATORS: { value: string; label: string }[] = [
  { value: "and", label: "and" },
  { value: "or", label: "or" },
  { value: "eq", label: "==" },
  { value: "ne", label: "!=" },
  { value: "gt", label: ">" },
  { value: "ge", label: ">=" },
  { value: "lt", label: "<" },
  { value: "le", label: "<=" },
  { value: "contains", label: "contains" },
  { value: "not_contains", label: "not contains" },
  { value: "matches", label: "=~" },
  { value: "not_matches", label: "not =~" },
  { value: "in", label: "in" },
  { value: "not_in", label: "not in" },
]

/** Why the node can't be deleted, or undefined when it can. Mirrors the bridge's rules. */
export function deleteBlocker(
  node: AstNode,
  byId: Map<string, AstNode>
): string | undefined {
  const dot = node.id.lastIndexOf(".")
  const parent = dot < 0 ? undefined : byId.get(node.id.slice(0, dot))
  if (!parent) return "The whole rule can’t be deleted. Edit the text instead."
  if (parent.kind === "unary") return undefined
  if (parent.kind === "array") {
    return (parent.children?.length ?? 0) > 1
      ? undefined
      : "A list needs at least one item."
  }
  if (
    parent.kind === "binary" &&
    (parent.operator === "and" || parent.operator === "or")
  )
    return undefined
  return "Comparison operands can’t be removed on their own. Select the comparison instead."
}

export type SyntaxError = { line: number; column: number; reason: string }

/** Parses `syntax error at line L:C:\n<source line>\n<caret>\n<reason>`. */
export function parseSyntaxError(message: string): SyntaxError | undefined {
  const match =
    /^syntax error at line (\d+):(\d+):\n[^\n]*\n[^\n]*\n([\s\S]+)$/.exec(
      message
    )
  if (!match) return undefined
  return {
    line: Number(match[1]),
    column: Number(match[2]),
    reason: match[3].trim(),
  }
}

/**
 * Ids of the and/or nodes that the source wraps in their own parentheses.
 * The AST drops parentheses, so this is what tells `(a and b) and c` (two
 * groups) from `a and b and c` (one).
 */
export function parenthesized(nodes: AstNode[], tokens: Token[]): Set<string> {
  // Match parentheses, then mark nodes sitting exactly inside a pair.
  const closeOf = new Map<number, number>()
  const open: number[] = []
  tokens.forEach((t, i) => {
    if (t.raw === "(") open.push(i)
    else if (t.raw === ")" && open.length) closeOf.set(open.pop()!, i)
  })
  const out = new Set<string>()
  for (const node of nodes) {
    if (
      node.kind !== "binary" ||
      (node.operator !== "and" && node.operator !== "or")
    )
      continue
    // Spans leave out a child group's own parentheses at either edge, as
    // in `(a and not (b))`, so skip the ones the node itself leaves open.
    let depth = 0
    let lowest = 0
    for (const t of tokens) {
      if (t.span.start < node.span.start || t.span.end > node.span.end) continue
      depth += t.raw === "(" ? 1 : t.raw === ")" ? -1 : 0
      lowest = Math.min(lowest, depth)
    }
    const before =
      tokens.findLastIndex((t) => t.span.end <= node.span.start) + lowest
    const first = tokens.findIndex((t) => t.span.start >= node.span.end)
    const after = first < 0 ? -1 : first + depth - lowest
    if (before >= 0 && closeOf.get(before) === after) out.add(node.id)
  }
  return out
}
