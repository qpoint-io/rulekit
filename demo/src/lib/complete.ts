/** One suggestion; `label` is inserted, `detail` is shown dimmed beside it. */
export type CompletionItem = {
  label: string
  detail?: string
  kind: "field" | "operator" | "keyword" | "value"
}

/** Suggestions for the text between `from` and the caret. */
export type Completion = { from: number; items: CompletionItem[] }

export type InputField = { path: string; detail: string }

/** Every dotted path in a JSON document, with a short description of its value. */
export function inputFields(json: string): InputField[] {
  let root: unknown
  try {
    root = JSON.parse(json)
  } catch {
    return []
  }
  const out: InputField[] = []
  const walk = (value: unknown, path: string) => {
    if (value && typeof value === "object" && !Array.isArray(value)) {
      for (const [key, child] of Object.entries(value)) {
        // Rulekit paths are dotted identifiers; other keys can't be referenced this way.
        if (/^[A-Za-z_]\w*$/.test(key))
          walk(child, path ? `${path}.${key}` : key)
      }
      return
    }
    const detail = Array.isArray(value) ? "list" : JSON.stringify(value)
    out.push({
      path,
      detail: detail.length > 24 ? `${detail.slice(0, 23)}…` : detail,
    })
  }
  walk(root, "")
  return out
}

const OPERATORS: CompletionItem[] = [
  { label: "==", detail: "equals", kind: "operator" },
  { label: "!=", detail: "not equal", kind: "operator" },
  { label: "in", detail: "in list or CIDR", kind: "operator" },
  { label: "not in", kind: "operator" },
  { label: "contains", kind: "operator" },
  { label: "not contains", kind: "operator" },
  { label: "=~", detail: "matches regex", kind: "operator" },
  { label: ">", kind: "operator" },
  { label: ">=", kind: "operator" },
  { label: "<", kind: "operator" },
  { label: "<=", kind: "operator" },
  { label: "matches", detail: "same as =~", kind: "operator" },
  { label: "not matches", kind: "operator" },
  { label: "eq", detail: "same as ==", kind: "operator" },
  { label: "ne", detail: "same as !=", kind: "operator" },
  { label: "gt", detail: "same as >", kind: "operator" },
  { label: "ge", detail: "same as >=", kind: "operator" },
  { label: "lt", detail: "same as <", kind: "operator" },
  { label: "le", detail: "same as <=", kind: "operator" },
]

const LOGIC: CompletionItem[] = [
  { label: "and", kind: "keyword" },
  { label: "or", kind: "keyword" },
]

const KEYWORDS = new Set([
  "and",
  "or",
  "not",
  "in",
  "contains",
  "matches",
  "eq",
  "ne",
  "gt",
  "ge",
  "lt",
  "le",
  "true",
  "false",
])

/**
 * What to suggest at `caret` in rule `source`: fields at the start of an
 * expression, operators after a field, and/or after a value.
 */
export function completeRule(
  source: string,
  caret: number,
  fields: InputField[]
): Completion | undefined {
  const before = source.slice(0, caret)
  // Inside a comment or an unterminated string: nothing to suggest.
  const line = before.slice(before.lastIndexOf("\n") + 1)
  if (line.includes("--") || (before.match(/"/g)?.length ?? 0) % 2 === 1)
    return undefined

  const prefix = /[A-Za-z_][\w.]*$|[=!<>~]+$/.exec(before)?.[0] ?? ""
  // With nothing typed yet, only offer the next token after a space: closing
  // a string or list shouldn't pop a menu.
  if (!prefix && !/(^|\s)$/.test(before)) return undefined
  const from = caret - prefix.length
  const prev = before.slice(0, from).trimEnd()
  const lastWord = /([A-Za-z_][\w.]*)$/.exec(prev)?.[1]

  let pool: CompletionItem[]
  if (lastWord && !KEYWORDS.has(lastWord.toLowerCase())) {
    pool = OPERATORS
  } else if (/(["\])/\d]|\btrue|\bfalse)$/.test(prev)) {
    pool = LOGIC
  } else if (
    /[=!<>~]$|\b(in|contains|matches|eq|ne|gt|ge|lt|le)$/i.test(prev)
  ) {
    // A value goes here: offer what the input currently has for this field.
    const field =
      /([A-Za-z_][\w.]*)\s*(==|!=|>=|<=|>|<|\b(?:contains|eq|ne|gt|ge|lt|le))$/i.exec(
        prev
      )?.[1]
    const current = fields.find((f) => f.path === field)
    if (!current || current.detail === "list" || current.detail.endsWith("…"))
      return undefined
    pool = [{ label: current.detail, detail: "in the input", kind: "value" }]
  } else {
    pool = [
      ...fields.map((f): CompletionItem => ({
        label: f.path,
        detail: f.detail,
        kind: "field",
      })),
      { label: "not", kind: "keyword" },
    ]
  }

  const p = prefix.toLowerCase()
  const items = [
    ...pool.filter((item) => item.label.toLowerCase().startsWith(p)),
    ...(p
      ? pool.filter(
          (item) =>
            !item.label.toLowerCase().startsWith(p) &&
            item.label.toLowerCase().includes(p)
        )
      : []),
  ].filter((item) => item.label !== prefix)
  return items.length ? { from, items: items.slice(0, 8) } : undefined
}
