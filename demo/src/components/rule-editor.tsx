import { useMemo } from "react"

import type { SyntaxError } from "@/lib/ast"
import { completeRule, type InputField } from "@/lib/complete"
import { lexRule, ruleSegments } from "@/lib/highlight"
import type { Span, Token } from "@/lib/rulekit"
import { offsetMap } from "@/lib/source-map"

import { CodeEditor, type Mark } from "./code-editor"

type Props = {
  source: string
  onChange(next: string): void
  /** Engine tokens for `source`; omitted while it doesn't parse. */
  tokens?: Token[]
  hoverSpan?: Span
  selectedSpan?: Span
  syntaxError?: SyntaxError
  /** Byte range under the caret or selection. */
  onCaret(start: number, end: number): void
  /** Input fields offered by autocomplete. */
  fields: InputField[]
}

/** Rule text editor. Rulekit speaks UTF-8 byte offsets; this converts at the edge. */
export function RuleEditor({
  source,
  onChange,
  tokens,
  hoverSpan,
  selectedSpan,
  syntaxError,
  onCaret,
  fields,
}: Props) {
  const map = useMemo(() => offsetMap(source), [source])
  const segments = useMemo(
    () =>
      tokens ? ruleSegments(source, tokens, map.toUtf16) : lexRule(source),
    [source, tokens, map]
  )

  const errorAt = useMemo(() => {
    if (!syntaxError) return undefined
    const lineStart = source
      .split("\n")
      .slice(0, syntaxError.line - 1)
      .reduce((n, line) => n + line.length + 1, 0)
    return map.toUtf16(map.toByte(lineStart) + syntaxError.column - 1)
  }, [syntaxError, source, map])

  const marks: Mark[] = []
  if (hoverSpan) {
    marks.push({
      start: map.toUtf16(hoverSpan.start),
      end: map.toUtf16(hoverSpan.end),
      className: "bg-foreground/[0.06]",
      link: "hover",
    })
  }
  if (selectedSpan) {
    marks.push({
      start: map.toUtf16(selectedSpan.start),
      end: map.toUtf16(selectedSpan.end),
      className: "bg-selection/15",
      link: "selected",
    })
  }

  return (
    <CodeEditor
      label="Rule source"
      value={source}
      onChange={onChange}
      segments={segments}
      marks={marks}
      errorAt={errorAt}
      onCaret={(start, end) => onCaret(map.toByte(start), map.toByte(end))}
      complete={(value, caret) => completeRule(value, caret, fields)}
    />
  )
}
