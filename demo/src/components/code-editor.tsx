import { createPortal } from "react-dom"
import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"

import type { Completion } from "@/lib/complete"
import type { Segment } from "@/lib/highlight"
import { cn } from "@/lib/utils"

// Every layer shares this so wrapped glyphs line up exactly with the textarea.
const TEXT =
  "m-0 py-4 pr-4 pl-12 font-mono text-[13px] leading-6 whitespace-pre-wrap [overflow-wrap:anywhere] [tab-size:2]"

/** A highlighted UTF-16 range, e.g. the selected node. `link` tags it for connector lines. */
export type Mark = {
  start: number
  end: number
  className: string
  link?: string
}

type Props = {
  label: string
  value: string
  onChange(next: string): void
  segments: Segment[]
  marks?: Mark[]
  /** UTF-16 offset of a syntax error; underlined, and its line number turns red. */
  errorAt?: number
  /** UTF-16 caret/selection range. */
  onCaret?(start: number, end: number): void
  /** Suggestions for the caret position; typing opens them, Ctrl+Space forces them. */
  complete?(value: string, caret: number): Completion | undefined
  className?: string
}

type Open = Completion & { caret: number; index: number }

export function paint(value: string, segments: Segment[]): ReactNode[] {
  const out: ReactNode[] = []
  let pos = 0
  for (const seg of segments) {
    if (seg.start < pos) continue
    if (seg.start > pos) out.push(value.slice(pos, seg.start))
    out.push(
      <span key={seg.start} className={seg.className}>
        {value.slice(seg.start, seg.end)}
      </span>
    )
    pos = seg.end
  }
  out.push(value.slice(pos))
  return out
}

/**
 * A textarea over a highlighted copy of its text. Soft-wraps; the outer box
 * scrolls vertically and the textarea grows with its content, so no layer
 * needs scroll syncing.
 */
export function CodeEditor({
  label,
  value,
  onChange,
  segments,
  marks = [],
  errorAt,
  onCaret,
  complete,
  className,
}: Props) {
  const painted = useMemo(() => paint(value, segments), [value, segments])
  const lines = useMemo(() => value.split("\n"), [value])
  const errorLine =
    errorAt === undefined ? -1 : value.slice(0, errorAt).split("\n").length - 1
  const textarea = useRef<HTMLTextAreaElement>(null)
  const anchor = useRef<HTMLSpanElement>(null)
  const [open, setOpen] = useState<Open>()
  const [at, setAt] = useState({ left: 0, top: 0 })
  const pendingCaret = useRef<number | undefined>(undefined)

  // Place the menu under the caret, measured from a mirror of the text. It
  // renders fixed in a portal so the editor's scroll box can't clip it.
  useLayoutEffect(() => {
    if (!open || !anchor.current) return
    const r = anchor.current.getBoundingClientRect()
    setAt({ left: r.left, top: r.top + 22 })
  }, [open])

  // Restore the caret after an accepted suggestion rewrites the value.
  useLayoutEffect(() => {
    if (pendingCaret.current === undefined || !textarea.current) return
    textarea.current.setSelectionRange(
      pendingCaret.current,
      pendingCaret.current
    )
    pendingCaret.current = undefined
  }, [value])

  const suggest = (next: string, caret: number) => {
    const found = complete?.(next, caret)
    setOpen(found ? { ...found, caret, index: 0 } : undefined)
  }

  const accept = (index: number) => {
    if (!open) return
    const item = open.items[index]
    const insert = `${item.label} `
    pendingCaret.current = open.from + insert.length
    setOpen(undefined)
    onChange(
      value.slice(0, open.from) +
        insert +
        value.slice(open.caret).replace(/^ /, "")
    )
  }

  return (
    <div
      className={cn(
        "min-h-0 flex-1 overflow-x-hidden overflow-y-auto rounded-lg bg-muted/40 ring-1 ring-border focus-within:ring-ring/60",
        className
      )}
      // Re-measure so the menu follows the caret when the editor scrolls.
      onScroll={() => setOpen((o) => o && { ...o })}
    >
      <div className="relative min-h-full">
        <pre
          aria-hidden
          className={cn(
            TEXT,
            "pointer-events-none absolute inset-0 text-transparent select-none"
          )}
        >
          {lines.map((line, i) => (
            <div key={i} className="relative">
              <span
                className={cn(
                  "absolute -left-10 w-6 text-right text-[11px] text-muted-foreground/60 tabular-nums",
                  i === errorLine && "font-semibold text-destructive"
                )}
              >
                {i + 1}
              </span>
              {line || " "}
            </div>
          ))}
        </pre>
        <div
          aria-hidden
          className="pointer-events-none absolute inset-y-0 left-0 w-9 border-r border-border/60"
        />
        {marks.map((mark) => (
          <pre
            key={mark.link ?? mark.className}
            aria-hidden
            className={cn(
              TEXT,
              "pointer-events-none absolute inset-0 text-transparent"
            )}
          >
            {value.slice(0, mark.start)}
            <mark
              data-link={mark.link}
              className={cn("rounded-[3px] text-transparent", mark.className)}
            >
              {value.slice(mark.start, mark.end)}
            </mark>
          </pre>
        ))}
        {errorAt !== undefined && (
          <pre
            aria-hidden
            className={cn(
              TEXT,
              "pointer-events-none absolute inset-0 text-transparent"
            )}
          >
            {value.slice(0, errorAt)}
            {/* Underline the whole offending word; a lone character reads as a tick mark. */}
            <mark className="bg-transparent text-transparent underline decoration-destructive decoration-wavy decoration-[1.5px] underline-offset-[5px] [text-decoration-skip-ink:none]">
              {/^[^\s]+/.exec(value.slice(errorAt))?.[0] ?? "\u00a0\u00a0"}
            </mark>
          </pre>
        )}
        <pre
          aria-hidden
          className={cn(TEXT, "pointer-events-none relative text-foreground")}
        >
          {painted}
          {/* Keeps a trailing newline's empty line tall enough for the caret. */}
          {"\n "}
        </pre>
        <textarea
          ref={textarea}
          aria-label={label}
          value={value}
          spellCheck={false}
          autoCapitalize="off"
          autoComplete="off"
          className={cn(
            TEXT,
            "absolute inset-0 size-full resize-none overflow-hidden bg-transparent text-transparent caret-foreground outline-none selection:bg-primary/30"
          )}
          onSelect={
            onCaret &&
            ((e) =>
              onCaret(
                e.currentTarget.selectionStart,
                e.currentTarget.selectionEnd
              ))
          }
          onChange={(e) => {
            onChange(e.target.value)
            const native = e.nativeEvent as InputEvent
            // Suggest while typing; deleting or pasting closes the menu.
            if (native.inputType === "insertText")
              suggest(e.target.value, e.target.selectionStart)
            else setOpen(undefined)
          }}
          onKeyDown={(e) => {
            if (complete && e.ctrlKey && e.key === " ") {
              e.preventDefault()
              suggest(value, e.currentTarget.selectionStart)
              return
            }
            if (!open) return
            const n = open.items.length
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault()
              const step = e.key === "ArrowDown" ? 1 : -1
              setOpen({ ...open, index: (open.index + step + n) % n })
            } else if (e.key === "Enter" || e.key === "Tab") {
              e.preventDefault()
              accept(open.index)
            } else if (e.key === "Escape") {
              e.preventDefault()
              e.nativeEvent.stopImmediatePropagation()
              setOpen(undefined)
            } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
              setOpen(undefined)
            }
          }}
          onBlur={() => setOpen(undefined)}
          aria-autocomplete={complete ? "list" : undefined}
          aria-expanded={complete ? !!open : undefined}
          aria-controls={open ? `${label}-completions` : undefined}
        />
        {open && (
          <>
            <pre
              aria-hidden
              className={cn(
                TEXT,
                "pointer-events-none invisible absolute inset-0"
              )}
            >
              {value.slice(0, open.from)}
              <span ref={anchor} />
            </pre>
            {createPortal(
              <ul
                id={`${label}-completions`}
                role="listbox"
                className="fixed z-50 max-w-80 min-w-48 overflow-hidden rounded-lg bg-popover p-1 text-popover-foreground shadow-lg ring-1 ring-foreground/10"
                style={{ left: at.left, top: at.top }}
              >
                {open.items.map((item, i) => (
                  <li
                    key={item.label}
                    role="option"
                    aria-selected={i === open.index}
                    className={cn(
                      "flex cursor-default items-baseline gap-3 rounded-md px-2 py-1 font-mono text-[13px]",
                      i === open.index && "bg-accent"
                    )}
                    // mousedown, not click: keep focus in the textarea.
                    onMouseDown={(e) => {
                      e.preventDefault()
                      accept(i)
                    }}
                  >
                    <span
                      className={cn(
                        "truncate",
                        item.kind === "field"
                          ? "text-tok-id"
                          : item.kind === "value"
                            ? /^"/.test(item.label)
                              ? "text-tok-str"
                              : "text-tok-num"
                            : item.kind === "keyword"
                              ? "font-medium text-tok-kw"
                              : /^[a-z]/.test(item.label)
                                ? "font-medium text-tok-kw"
                                : "text-tok-op"
                      )}
                    >
                      {item.label}
                    </span>
                    {item.detail && (
                      <span className="ml-auto truncate font-sans text-xs text-muted-foreground">
                        {item.detail}
                      </span>
                    )}
                  </li>
                ))}
              </ul>,
              document.body
            )}
          </>
        )}
      </div>
    </div>
  )
}
