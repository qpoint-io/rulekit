import { useEffect, useState } from "react"

type Point = { x: number; y: number; nx: number; ny: number }
type Line = { d: string; from: Point; to: Point }

/** True when `rect` is inside every clipping ancestor of `el`, so it's actually on screen. */
function onScreen(el: Element, rect: DOMRect) {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const style = getComputedStyle(p)
    if (style.overflowX === "visible" && style.overflowY === "visible") continue
    const clip = p.getBoundingClientRect()
    const cy = (rect.top + rect.bottom) / 2
    const cx = (rect.left + rect.right) / 2
    if (cy < clip.top || cy > clip.bottom || cx < clip.left || cx > clip.right)
      return false
  }
  return true
}

/** Distance kept between a line's end and the element it points at. */
const GAP = 5

/** Side midpoints with outward normals. */
function anchors(r: DOMRect): Point[] {
  const cx = (r.left + r.right) / 2
  const cy = (r.top + r.bottom) / 2
  return [
    { x: r.right, y: cy, nx: 1, ny: 0 },
    { x: r.left, y: cy, nx: -1, ny: 0 },
    { x: cx, y: r.bottom, nx: 0, ny: 1 },
    { x: cx, y: r.top, nx: 0, ny: -1 },
  ]
}

/** Curve between the closest pair of facing sides of two boxes. */
function connect(a: DOMRect, b: DOMRect): Line {
  let best: [Point, Point] | undefined
  let bestScore = Infinity
  for (const p of anchors(a)) {
    for (const q of anchors(b)) {
      const dx = q.x - p.x
      const dy = q.y - p.y
      // Only sides that face each other.
      if (dx * p.nx + dy * p.ny < 0 || dx * q.nx + dy * q.ny > 0) continue
      const score = Math.hypot(dx, dy)
      if (score < bestScore) {
        bestScore = score
        best = [p, q]
      }
    }
  }
  const [p, q] = best ?? [anchors(a)[0], anchors(b)[1]]
  // Stop just short of each box so the dot never sits on its content.
  const from = { ...p, x: p.x + p.nx * GAP, y: p.y + p.ny * GAP }
  const to = { ...q, x: q.x + q.nx * GAP, y: q.y + q.ny * GAP }
  const pull = Math.max(24, Math.hypot(to.x - from.x, to.y - from.y) / 2.5)
  const d = `M ${from.x} ${from.y} C ${from.x + from.nx * pull} ${from.y + from.ny * pull}, ${to.x + to.nx * pull} ${to.y + to.ny * pull}, ${to.x} ${to.y}`
  return { d, from, to }
}

function visibleRect(el: Element | null): [Element, DOMRect] | undefined {
  // A wrapped mark has one rect per line; anchor to the first.
  const rect = el?.getClientRects()[0]
  return el && rect && rect.width > 0 && onScreen(el, rect)
    ? [el, rect]
    : undefined
}

/**
 * Draws curves joining one node's appearances across panels, e.g. its text in
 * the rule editor, its entry in the structure view, and its input field.
 * Each consecutive pair of visible elements in `chain` gets a curve; a
 * missing or scrolled-away hop breaks the chain there. A hop may list
 * fallbacks, `"a, b"` style: the first selector that matches is used.
 */
export function LinkLines({
  chain,
  tone,
}: {
  /** One entry per hop; an array lists fallback selectors, first match wins. */
  chain: (string | string[])[]
  tone: "selected" | "hover"
}) {
  const [lines, setLines] = useState<Line[]>([])
  const selectors = chain
    .map((hop) => (Array.isArray(hop) ? hop.join("\t") : hop))
    .join("\n")

  useEffect(() => {
    if (!selectors) return
    let frame = 0
    // Undefined, not "", so the first frame always paints: an empty chain must
    // clear lines left over from the previous one.
    let last: string | undefined
    // Panels scroll and resize independently; one rAF loop tracks all of it.
    const tick = () => {
      const rects = selectors.split("\n").map((hop) => {
        const el = hop
          .split("\t")
          .map((sel) => document.querySelector(sel))
          .find(Boolean)
        return visibleRect(el ?? null)
      })
      const next: Line[] = []
      for (let i = 1; i < rects.length; i++) {
        const a = rects[i - 1]
        const b = rects[i]
        if (a && b) next.push(connect(a[1], b[1]))
      }
      const key = next.map((l) => l.d).join("|")
      if (key !== last) {
        last = key
        setLines(next)
      }
      frame = requestAnimationFrame(tick)
    }
    tick()
    return () => cancelAnimationFrame(frame)
  }, [selectors])

  if (!selectors || !lines.length) return null
  const color =
    tone === "selected" ? "var(--selection)" : "var(--muted-foreground)"
  return (
    <svg aria-hidden className="pointer-events-none fixed inset-0 size-full">
      {lines.map((line) => (
        <g key={line.d}>
          <path
            d={line.d}
            fill="none"
            stroke={color}
            strokeWidth={1}
            strokeOpacity={tone === "selected" ? 0.45 : 0.3}
            strokeDasharray={tone === "hover" ? "4 4" : undefined}
          />
          <circle
            cx={line.from.x}
            cy={line.from.y}
            r={1.75}
            fill={color}
            fillOpacity={0.6}
          />
          <circle
            cx={line.to.x}
            cy={line.to.y}
            r={1.75}
            fill={color}
            fillOpacity={0.6}
          />
        </g>
      ))}
    </svg>
  )
}
