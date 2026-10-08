import type { Token } from "./rulekit"

/** A styled UTF-16 range of editor text. Text between segments renders unstyled. */
export type Segment = { start: number; end: number; className: string }

const COMMENT = "text-tok-comment italic"

/** Class for one rulekit token. The engine's `role` is coarse, so refine by `kind`. */
function ruleTokenClass(token: Pick<Token, "kind" | "role" | "raw">): string {
  switch (token.kind) {
    case "BOOL":
      return "text-tok-const"
    case "REGEX":
      return "text-tok-regex"
    case "IP":
    case "IP_CIDR":
    case "HEX_STRING":
      return "text-tok-num"
  }
  if (token.role === "kw") {
    return /^[a-z]/i.test(token.raw) ? "font-medium text-tok-kw" : "text-tok-op"
  }
  if (token.role === "id") return "text-tok-id"
  if (token.role === "str") return "text-tok-str"
  if (token.role === "num") return "text-tok-num"
  return "text-tok-pun"
}

const COMMENT_RE = /--[^\n]*|\/\*[\s\S]*?(?:\*\/|$)/g

/** Comments aren't tokens, so find them in the gaps between tokens. */
function commentsIn(source: string, from: number, to: number, out: Segment[]) {
  const gap = source.slice(from, to)
  for (const m of gap.matchAll(COMMENT_RE)) {
    out.push({
      start: from + m.index,
      end: from + m.index + m[0].length,
      className: COMMENT,
    })
  }
}

/** Segments from the engine's tokens for a rule that parsed. */
export function ruleSegments(
  source: string,
  tokens: Token[],
  toUtf16: (byte: number) => number
): Segment[] {
  const out: Segment[] = []
  let pos = 0
  for (const token of tokens) {
    const start = toUtf16(token.span.start)
    const end = toUtf16(token.span.end)
    if (start < pos) continue
    commentsIn(source, pos, start, out)
    out.push({ start, end, className: ruleTokenClass(token) })
    pos = end
  }
  commentsIn(source, pos, source.length, out)
  return out
}

// Approximates the rulekit lexer so a rule mid-edit stays colored while it doesn't parse.
const RULE_LEX = new RegExp(
  [
    String.raw`(?<comment>--[^\n]*|\/\*[\s\S]*?(?:\*\/|$))`,
    String.raw`(?<str>"(?:[^"\\\n]|\\.)*"?|'(?:[^'\\\n]|\\.)*'?)`,
    String.raw`(?<regex>(?<=(?:=~|matches)\s*)\/(?:[^/\\\n]|\\.)*\/?[a-z]*)`,
    String.raw`(?<ip>\b\d{1,3}(?:\.\d{1,3}){3}(?:\/\d+)?\b)`,
    String.raw`(?<num>-?\b(?:0x[0-9a-f]+|\d+(?:\.\d+)?)\b)`,
    String.raw`(?<bool>\b(?:true|false)\b)`,
    String.raw`(?<kw>\b(?:and|or|not|in|contains|matches|eq|ne|gt|ge|lt|le)\b)`,
    String.raw`(?<op>==|!=|>=|<=|=~|&&|\|\||[<>!])`,
    String.raw`(?<id>[A-Za-z_][\w.]*)`,
    String.raw`(?<pun>[()[\],])`,
  ].join("|"),
  "gi"
)

const FALLBACK_CLASS: Record<string, string> = {
  comment: COMMENT,
  str: "text-tok-str",
  regex: "text-tok-regex",
  ip: "text-tok-num",
  num: "text-tok-num",
  bool: "text-tok-const",
  kw: "font-medium text-tok-kw",
  op: "text-tok-op",
  id: "text-tok-id",
  pun: "text-tok-pun",
}

/** Best-effort segments for a rule that doesn't parse. */
export function lexRule(source: string): Segment[] {
  return lexWith(RULE_LEX, FALLBACK_CLASS, source)
}

const JSON_LEX =
  /(?<key>"(?:[^"\\\n]|\\.)*"(?=\s*:))|(?<str>"(?:[^"\\\n]|\\.)*"?)|(?<num>-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(?<const>\b(?:true|false|null)\b)|(?<pun>[{}[\],:])/g

const JSON_CLASS: Record<string, string> = {
  key: "text-tok-id",
  str: "text-tok-str",
  num: "text-tok-num",
  const: "text-tok-const",
  pun: "text-tok-pun",
}

export function lexJson(source: string): Segment[] {
  return lexWith(JSON_LEX, JSON_CLASS, source)
}

function lexWith(
  re: RegExp,
  classes: Record<string, string>,
  source: string
): Segment[] {
  const out: Segment[] = []
  for (const m of source.matchAll(re)) {
    const group = Object.entries(m.groups ?? {}).find(
      ([, text]) => text !== undefined
    )?.[0]
    if (group)
      out.push({
        start: m.index,
        end: m.index + m[0].length,
        className: classes[group],
      })
  }
  return out
}

/** UTF-16 range of the key at `path` (e.g. `["dst", "port"]`) in JSON text, if present. */
export function findJsonKey(
  source: string,
  path: string[]
): { start: number; end: number } | undefined {
  // Each open container records the key it hangs off; arrays break the path.
  const keys: string[] = []
  const kinds: ("{" | "[")[] = []
  let pending = "\0"
  for (const m of source.matchAll(/"(?:[^"\\\n]|\\.)*"(\s*:)?|[{}[\]]/g)) {
    const text = m[0]
    if (text === "{" || text === "[") {
      keys.push(kinds.at(-1) === "{" ? pending : "\0")
      kinds.push(text)
    } else if (text === "}" || text === "]") {
      keys.pop()
      kinds.pop()
    } else if (m[1] !== undefined && kinds.at(-1) === "{") {
      const quoted = text.slice(0, text.length - m[1].length)
      try {
        pending = JSON.parse(quoted) as string
      } catch {
        pending = "\0"
      }
      // keys[0] is the root object's (nonexistent) parent key.
      const here = [...keys.slice(1), pending]
      if (here.length === path.length && here.every((k, i) => k === path[i])) {
        return { start: m.index, end: m.index + quoted.length }
      }
    }
  }
  return undefined
}
