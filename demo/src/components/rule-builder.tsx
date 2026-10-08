import {
  createContext,
  use,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react"
import {
  BanIcon,
  CopyIcon,
  GripVerticalIcon,
  ListPlusIcon,
  PlusIcon,
  Trash2Icon,
} from "lucide-react"

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { parenthesized, spanKey } from "@/lib/ast"
import type { InputField } from "@/lib/complete"
import { lexRule } from "@/lib/highlight"
import {
  fromAst,
  find,
  insertAt,
  move,
  print,
  update,
  type RuleModel,
} from "@/lib/rule-model"
import { flattenAST, parseRule, type AstNode } from "@/lib/rulekit"
import { cn } from "@/lib/utils"

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

import { paint } from "./code-editor"
import { STATUS } from "./status"
import type { StructureProps } from "./structure-tree"

/** Comparison operators as they're written in a rule. */
/** Comparison operators, each with every spelling rulekit accepts for it. */
const OPERATOR_GROUPS: { spellings: string[]; meaning: string }[] = [
  { spellings: ["==", "eq"], meaning: "equals" },
  { spellings: ["!=", "ne"], meaning: "not equal" },
  { spellings: [">", "gt"], meaning: "greater than" },
  { spellings: [">=", "ge"], meaning: "at least" },
  { spellings: ["<", "lt"], meaning: "less than" },
  { spellings: ["<=", "le"], meaning: "at most" },
  { spellings: ["in"], meaning: "in a list or CIDR" },
  { spellings: ["not in"], meaning: "not in" },
  { spellings: ["contains"], meaning: "has substring or item" },
  { spellings: ["not contains"], meaning: "lacks" },
  { spellings: ["=~", "matches"], meaning: "matches a regex" },
  { spellings: ["not matches", "not =~"], meaning: "doesn’t match" },
]
const SPELLINGS = new Set(OPERATOR_GROUPS.flatMap((g) => g.spellings))

const JOIN_ITEMS = [
  { value: "and", label: "and" },
  { value: "or", label: "or" },
]

/** A condition split into the parts the row edits; `undefined` when it isn't `field op value`. */
type Parts = { field: string; op: string; value: string }

function partsOf(node: AstNode | undefined): Parts | undefined {
  const [lhs, rhs] = node?.children ?? []
  if (!node || node.kind !== "binary" || !lhs || !rhs || lhs.kind !== "path")
    return undefined
  // Keep the spelling the rule uses (`ne` stays `ne`).
  const op = node.raw?.toLowerCase().replace(/\s+/g, " ") ?? "=="
  return { field: lhs.path || lhs.text, op, value: rhs.text }
}

/**
 * The condition `parts` with operator `op`, adapted so it means what was
 * picked rather than failing to parse:
 * - `not <op>` becomes `<op>` wrapped in a negation, since only `in`,
 *   `contains` and `matches` have a `not` form and a negation node shows up
 *   (and can be removed) in the builder;
 * - `in` with a single value puts it in a list, `["dev"]`;
 * - `=~` / `matches` with a quoted string turns it into a regex.
 */
function withOperator(
  parts: Parts,
  op: string
): { text: string; negate: boolean } {
  let base = op.trim().toLowerCase().replace(/\s+/g, " ")
  const negate = base.startsWith("not ")
  if (negate) base = base.slice(4)
  let value = parts.value.trim()
  const isList = value.startsWith("[")
  const isCidr = /^[0-9a-f:.]+\/\d+$/i.test(value)
  if (base === "in" && !isList && !isCidr) value = `[${value}]`
  if (
    (base === "=~" || base === "matches") &&
    /^"(?:[^"\\]|\\.)*"$/.test(value)
  ) {
    const literal = JSON.parse(value) as string
    value = `/${literal.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&")}/`
  }
  // A list only fits `in`; other operators compare against its first item.
  if (base !== "in" && isList) {
    const first = value.slice(1, -1).split(",")[0]?.trim()
    if (first) value = first
  }
  return { text: `${parts.field} ${base} ${value}`, negate }
}

/**
 * Change the one and/or before item `index` of `group`. Only that joint
 * changes: its two neighbors become their own parenthesized group with the
 * new operator, so `a and b and c` with the first `and` made `or` reads
 * `(a or b) and c`. The text is re-parsed, and a group whose operator now
 * matches its parent's is merged back in, so switching back undoes the split.
 */
async function switchJoint(
  ctx: Ctx,
  group: Extract<RuleModel, { kind: "group" }>,
  index: number,
  op: "and" | "or"
) {
  const part = (item: RuleModel) =>
    item.kind === "group" ? `(${print(item)})` : print(item)
  const parts = group.items.map(part)
  const text =
    group.items.length === 2
      ? `${parts[0]} ${op} ${parts[1]}`
      : [
          ...parts.slice(0, index - 1),
          `(${parts[index - 1]} ${op} ${parts[index]})`,
          ...parts.slice(index + 1),
        ].join(` ${group.op} `)
  const res = await parseRule(text)
  if (!res.ok || !res.ast) return
  const next = fromAst(
    res.ast,
    parenthesized(flattenAST(res.ast), res.tokens ?? [])
  )
  const parent = findParent(ctx.model, group.id)
  if (parent && next.kind === "group" && next.op === parent.op) {
    // Same operator as the parent: the parentheses mean nothing, so merge.
    ctx.apply(
      `change the and/or before item ${index + 1} to "${op}" in: ${print(group)}`,
      update(ctx.model, parent.id, (p) =>
        p.kind === "group"
          ? {
              ...p,
              items: p.items.flatMap((item) =>
                item.id === group.id ? next.items : [item]
              ),
            }
          : p
      )
    )
    return
  }
  ctx.apply(
    `change the and/or before item ${index + 1} to "${op}" in: ${print(group)}`,
    update(ctx.model, group.id, () => next)
  )
}

/** The `not` node directly wrapping node `id`, if any. */
function negationOf(model: RuleModel, id: string): RuleModel | undefined {
  if (model.kind === "not")
    return model.item.id === id ? model : negationOf(model.item, id)
  if (model.kind === "group") {
    for (const item of model.items) {
      const hit = negationOf(item, id)
      if (hit) return hit
    }
  }
  return undefined
}

type Drop = { groupId: string; index: number }

type Ctx = StructureProps & {
  model: RuleModel
  byId: Map<string, AstNode>
  fields: InputField[]
  /** Re-print the rule from `next`; `action` describes the change for the log. */
  apply(action: string, next: RuleModel | undefined): void
  /** A condition new rows start from, built from the input. */
  seed: string
  dragging?: string
  setDragging(id?: string): void
  drop?: Drop
  setDrop(drop?: Drop): void
  /** Show evaluation results (status icons, dimming, value cards). */
  preview: boolean
  /** Which combobox's list is open; only one may be at a time. */
  openList?: string
  setOpenList(id?: string): void
  /** The innermost row under the pointer; only its handle shows. */
  hoverRow?: string
  setHoverRow(id?: string): void
}

const BuilderCtx = createContext<Ctx | null>(null)
const useCtx = () => use(BuilderCtx)!

/**
 * The rule as a filter builder, after Notion's and Linear's advanced filters:
 * each group lists its conditions with one and/or for the whole group, every
 * condition is a field / operator / value control, and each group ends with
 * "Add condition". Rows can be dragged between groups.
 */
export function RuleBuilder({
  node,
  byId,
  fields,
  onReplace,
  preview,
  ...props
}: StructureProps & {
  preview: boolean
  byId: Map<string, AstNode>
  fields: InputField[]
  onReplace(next: string, action: string): void
}) {
  const model = useMemo(
    () => fromAst(node, props.grouped),
    [node, props.grouped]
  )
  const [dragging, setDragging] = useState<string>()
  const [drop, setDrop] = useState<Drop>()
  const [hoverRow, setHoverRow] = useState<string>()
  const [openList, setOpenList] = useState<string>()
  // Any press outside the open field/operator box closes its list, even on
  // things that don't take focus (so no blur fires).
  useEffect(() => {
    if (!openList) return
    const close = (e: PointerEvent) => {
      if (!(e.target as Element).closest("[data-combobox]"))
        setOpenList(undefined)
    }
    document.addEventListener("pointerdown", close, true)
    return () => document.removeEventListener("pointerdown", close, true)
  }, [openList])
  const leaf = fields.find(
    (f) => f.detail !== "list" && !f.detail.endsWith("…")
  )
  // The root is always shown as a group, so a one-condition rule still gets
  // the group's controls.
  const root: RuleModel =
    model.kind === "group"
      ? model
      : { kind: "group", id: "\u0000root", op: "and", items: [model] }
  const ctx: Ctx = {
    node,
    ...props,
    model: root,
    byId,
    fields,
    apply: (action, next) => onReplace(next ? print(next) : "", action),
    seed: leaf ? `${leaf.path} == ${leaf.detail}` : 'field == "value"',
    dragging,
    setDragging,
    drop,
    setDrop,
    hoverRow,
    setHoverRow,
    openList,
    setOpenList,
    preview,
  }
  return (
    <BuilderCtx value={ctx}>
      <div
        data-keep-selection
        className="p-2 text-sm"
        onMouseLeave={() => setHoverRow(undefined)}
      >
        <Group group={root} />
      </div>
    </BuilderCtx>
  )
}

function Group({ group }: { group: Extract<RuleModel, { kind: "group" }> }) {
  const ctx = useCtx()
  const add = (item: RuleModel) =>
    ctx.apply(
      `add ${item.kind === "group" ? "group" : "condition"}: ${print(item)}`,
      insertAt(ctx.model, group.id, group.items.length, item)
    )
  const cond = (): RuleModel => ({
    kind: "cond",
    id: `new:${Math.random()}`,
    text: ctx.seed,
  })
  return (
    <div
      className="flex flex-col gap-1.5"
      // Dropping is decided per group: the innermost group under the pointer
      // takes the drop, at the slot nearest the pointer among its own rows.
      // So a row's left gutter (and/or, handle) or a group's "Add condition"
      // line targets that group, including the top level.
      onDragOver={(e) => {
        if (!ctx.dragging) return
        e.preventDefault()
        e.stopPropagation()
        const rows = [
          ...e.currentTarget.querySelectorAll(":scope > [data-row]"),
        ]
        const at = rows.findIndex((row) => {
          const r = row.getBoundingClientRect()
          return e.clientY < r.top + r.height / 2
        })
        const index = at < 0 ? rows.length : at
        if (ctx.drop?.groupId !== group.id || ctx.drop.index !== index)
          ctx.setDrop({ groupId: group.id, index })
      }}
      onDrop={(e) => {
        e.preventDefault()
        e.stopPropagation()
        if (ctx.dragging && ctx.drop) {
          const where =
            ctx.drop.groupId === ctx.model.id
              ? "the top level"
              : `the group "${print(find(ctx.model, ctx.drop.groupId) ?? ctx.model)}"`
          ctx.apply(
            `drag "${print(find(ctx.model, ctx.dragging) ?? ctx.model)}" to position ${ctx.drop.index + 1} of ${where}`,
            move(ctx.model, ctx.dragging, ctx.drop.groupId, ctx.drop.index)
          )
        }
        ctx.setDragging(undefined)
        ctx.setDrop(undefined)
      }}
    >
      {group.items.map((item, i) => (
        <Row key={item.id} item={item} group={group} index={i} />
      ))}
      <DropZone groupId={group.id} index={group.items.length} />
      <div className="flex items-center gap-2">
        <span className="w-[5.5rem] shrink-0" />
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <button
                type="button"
                className="flex h-7 items-center gap-1.5 rounded-md px-2 text-muted-foreground hover:bg-accent hover:text-foreground data-popup-open:bg-accent"
              />
            }
          >
            <PlusIcon className="size-3.5" />
            Add condition
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="min-w-44">
            <DropdownMenuGroup>
              <DropdownMenuItem onClick={() => add(cond())}>
                <PlusIcon />
                Condition
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={() =>
                  add({
                    kind: "group",
                    id: `new-group:${Math.random()}`,
                    op: group.op === "and" ? "or" : "and",
                    items: [cond(), cond()],
                  })
                }
              >
                <ListPlusIcon />
                Group
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  )
}

/** The and/or column: "Where" first, the group's operator picker second, the word after. */
function Joiner({
  group,
  index,
}: {
  group: Extract<RuleModel, { kind: "group" }>
  index: number
}) {
  const ctx = useCtx()
  if (index === 0)
    return (
      <span className="w-[4.5rem] shrink-0 pt-1 pl-2 text-muted-foreground in-data-[align=center]:pt-0">
        Where
      </span>
    )
  return (
    <Select
      items={JOIN_ITEMS}
      value={group.op}
      onValueChange={(op) =>
        op && void switchJoint(ctx, group, index, op as "and" | "or")
      }
    >
      <SelectTrigger
        size="sm"
        className="mr-2 w-16 shrink-0 gap-0.5 pr-1 pl-2 font-mono text-tok-kw *:data-[slot=select-value]:overflow-visible"
        aria-label="Combine conditions with"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {JOIN_ITEMS.map((j) => (
            <SelectItem key={j.value} value={j.value} className="font-mono">
              {j.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

function Row({
  item,
  group,
  index,
}: {
  item: RuleModel
  group: Extract<RuleModel, { kind: "group" }>
  index: number
}) {
  const ctx = useCtx()
  const negated = item.kind === "not"
  const inner = item.kind === "not" ? item.item : item
  const draggingThis = ctx.dragging === item.id
  return (
    <>
      <DropZone groupId={group.id} index={index} />
      <div
        data-row
        data-align={inner.kind === "group" ? "start" : "center"}
        className={cn(
          "flex gap-1",
          inner.kind === "group" ? "items-start" : "items-center",
          draggingThis && "opacity-40"
        )}
        onMouseOver={(e) => {
          e.stopPropagation()
          if (ctx.hoverRow !== item.id) ctx.setHoverRow(item.id)
        }}
      >
        <Joiner group={group} index={index} />
        <div className="flex min-w-0 flex-1 items-start gap-1.5 in-data-[align=center]:items-center">
          {/* The node's own handle, right beside it: drag to move, click for actions. */}
          <RowMenu item={item} />
          {negated && (
            <button
              type="button"
              title="Remove not"
              className="mt-1 rounded px-1 font-mono text-tok-kw hover:bg-accent in-data-[align=center]:mt-0"
              onClick={() =>
                ctx.apply(
                  `remove not from: ${print(item)}`,
                  update(ctx.model, item.id, (m) =>
                    m.kind === "not" ? m.item : m
                  )
                )
              }
            >
              not
            </button>
          )}
          {inner.kind === "group" ? (
            <div className="min-w-0 flex-1 rounded-lg border border-border bg-foreground/[0.015] p-2">
              <Group group={inner} />
            </div>
          ) : inner.kind === "cond" ? (
            <Condition item={inner} />
          ) : (
            <span className="mt-1 font-mono text-[13px]">{print(inner)}</span>
          )}
        </div>
      </div>
    </>
  )
}

/** A thin line showing where a dragged row will land. */
function DropZone({ groupId, index }: { groupId: string; index: number }) {
  const ctx = useCtx()
  const active =
    ctx.dragging && ctx.drop?.groupId === groupId && ctx.drop.index === index
  return (
    <div
      aria-hidden
      className={cn(
        "-my-[3px] ml-[5.75rem] h-0.5 rounded-full",
        active ? "bg-primary" : "bg-transparent"
      )}
    />
  )
}

function RowMenu({ item }: { item: RuleModel }) {
  const ctx = useCtx()
  const handle = useRef<HTMLButtonElement>(null)
  const [open, setOpen] = useState(false)
  // A plain button rather than a menu trigger: menu triggers open on
  // mousedown (and modally), which would swallow the drag. The menu opens on
  // a click, which a drag never produces.
  return (
    <>
      <button
        ref={handle}
        type="button"
        draggable
        aria-label="Drag to move, click for actions"
        aria-haspopup="menu"
        aria-expanded={open}
        title="Drag to move · click for actions"
        data-popup-open={open || undefined}
        className={cn(
          "mt-1 grid h-6 w-4 shrink-0 cursor-grab place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:opacity-100 active:cursor-grabbing in-data-[align=center]:mt-0 data-popup-open:bg-accent data-popup-open:opacity-100",
          ctx.hoverRow === item.id || ctx.dragging === item.id
            ? "opacity-100"
            : "opacity-0"
        )}
        onClick={() => setOpen(true)}
        onDragStart={(e) => {
          e.dataTransfer.effectAllowed = "move"
          e.stopPropagation()
          setOpen(false)
          ctx.setDragging(item.id)
        }}
        onDragEnd={() => {
          ctx.setDragging(undefined)
          ctx.setDrop(undefined)
        }}
      >
        <GripVerticalIcon className="size-3.5" />
      </button>
      <DropdownMenu open={open} onOpenChange={setOpen}>
        <DropdownMenuContent align="start" className="min-w-44" anchor={handle}>
          <DropdownMenuGroup>
            <DropdownMenuItem
              onClick={() =>
                ctx.apply(
                  `${item.kind === "not" ? "remove not from" : "negate"}: ${print(item)}`,
                  update(ctx.model, item.id, (m) =>
                    m.kind === "not"
                      ? m.item
                      : { kind: "not", id: `not:${m.id}`, item: m }
                  )
                )
              }
            >
              <BanIcon />
              {item.kind === "not" ? "Remove not" : "Negate"}
            </DropdownMenuItem>
            <DropdownMenuItem
              onClick={() => {
                const parent = findParent(ctx.model, item.id)
                if (!parent) return
                const index = parent.items.findIndex((i) => i.id === item.id)
                ctx.apply(
                  `duplicate: ${print(item)}`,
                  insertAt(ctx.model, parent.id, index + 1, {
                    ...item,
                    id: `copy:${Math.random()}`,
                  })
                )
              }}
            >
              <CopyIcon />
              Duplicate
            </DropdownMenuItem>
            {item.kind !== "group" && (
              <DropdownMenuItem
                onClick={() =>
                  ctx.apply(
                    `turn into group: ${print(item)}`,
                    update(ctx.model, item.id, (m) => ({
                      kind: "group",
                      id: `wrap:${m.id}`,
                      op: "or",
                      items: [
                        m,
                        {
                          kind: "cond",
                          id: `new:${Math.random()}`,
                          text: ctx.seed,
                        },
                      ],
                    }))
                  )
                }
              >
                <ListPlusIcon />
                Turn into group
              </DropdownMenuItem>
            )}
          </DropdownMenuGroup>
          <DropdownMenuSeparator />
          <DropdownMenuGroup>
            <DropdownMenuItem
              variant="destructive"
              onClick={() =>
                ctx.apply(
                  `delete: ${print(item)}`,
                  update(ctx.model, item.id, () => undefined)
                )
              }
            >
              <Trash2Icon />
              Delete
            </DropdownMenuItem>
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  )
}

function findParent(
  model: RuleModel,
  id: string
): Extract<RuleModel, { kind: "group" }> | undefined {
  if (model.kind === "not") return findParent(model.item, id)
  if (model.kind !== "group") return undefined
  if (model.items.some((i) => i.id === id)) return model
  for (const item of model.items) {
    const hit = findParent(item, id)
    if (hit) return hit
  }
  return undefined
}

/** One condition: a segmented field / operator / value control with its result. */
function Condition({ item }: { item: Extract<RuleModel, { kind: "cond" }> }) {
  const ctx = useCtx()
  const node = ctx.byId.get(item.id)
  const parts = partsOf(node)
  const trace = node ? ctx.traceBySpan.get(spanKey(node.span)) : undefined
  const status = trace ? STATUS[trace.status] : undefined
  const pruned = ctx.dimmed || trace?.status === "pruned"
  // With preview on, the field's resolved value sits under it as a subtitle.
  const fieldTrace = node?.children?.[0]
    ? ctx.traceBySpan.get(spanKey(node.children[0].span))
    : undefined
  const subtitle =
    ctx.preview && fieldTrace && fieldTrace.status !== "pruned"
      ? fieldTrace.missingFields?.length
        ? "missing"
        : fieldTrace.value !== undefined
          ? JSON.stringify(fieldTrace.value)
          : undefined
      : undefined
  const setText = (text: string) =>
    ctx.apply(
      `edit "${item.text}" to "${text}"`,
      update(ctx.model, item.id, (m) => ({ ...m, text }))
    )
  const selected =
    ctx.selectedId === item.id ||
    (!!ctx.selectedId && ctx.selectedId.startsWith(`${item.id}.`))

  const pill = (
    <div
      data-node-id={item.id}
      className={cn(
        "flex min-h-9 min-w-0 flex-wrap items-center rounded-md border bg-card",
        selected ? "border-selection/70" : "border-border",
        ctx.preview && pruned && "opacity-55"
      )}
      onMouseEnter={() => node && ctx.onHover(node)}
      onMouseLeave={() => ctx.onHover()}
      onClick={() => node && ctx.selectedId !== node.id && ctx.onSelect(node)}
    >
      {ctx.preview && (
        <span className="grid w-7 shrink-0 place-items-center border-r">
          {status && (
            <status.icon
              aria-label={status.label}
              className={cn("size-3.5", status.tone)}
            />
          )}
        </span>
      )}
      {parts ? (
        <>
          <FieldInput
            value={parts.field}
            subtitle={subtitle}
            onCommit={(field) => setText(`${field} ${parts.op} ${parts.value}`)}
          />
          <OperatorInput
            value={parts.op}
            validate={(op) => withOperator(parts, op).text}
            onCommit={(op) => {
              const { text, negate } = withOperator(parts, op)
              const cond: RuleModel = { kind: "cond", id: item.id, text }
              // A `not` on an already negated condition cancels the negation.
              const wrapper = negationOf(ctx.model, item.id)
              ctx.apply(
                `set operator "${op}" on: ${item.text}`,
                negate && wrapper
                  ? update(ctx.model, wrapper.id, () => cond)
                  : update(ctx.model, item.id, () =>
                      negate
                        ? { kind: "not", id: `not:${item.id}`, item: cond }
                        : cond
                    )
              )
            }}
          />
          <TextPart
            label="Value"
            value={parts.value}
            validate={(value) => `${parts.field} ${parts.op} ${value}`}
            onCommit={(value) => setText(`${parts.field} ${parts.op} ${value}`)}
          />
        </>
      ) : (
        <TextPart
          label="Condition"
          value={item.text}
          validate={(t) => t}
          onCommit={setText}
        />
      )}
    </div>
  )
  return pill
}

/**
 * Highlighting for a value, lexed as part of its whole condition so context
 * matters (a regex is only a regex after `=~`). `full` ends with `value`.
 */
function valueSegments(full: string, value: string) {
  const offset = full.length - value.length
  return lexRule(full)
    .filter((seg) => seg.end > offset)
    .map((seg) => ({
      ...seg,
      start: Math.max(0, seg.start - offset),
      end: seg.end - offset,
    }))
}

/** A borderless text segment that commits on Enter or blur if the result parses. */
function TextPart({
  label,
  value,
  validate,
  onCommit,
}: {
  label: string
  value: string
  /** The full condition text this value would produce, checked before committing. */
  validate(value: string): string
  onCommit(value: string): void
}) {
  const [draft, setDraft] = useState(value)
  const [error, setError] = useState<string>()
  const [last, setLast] = useState(value)
  if (value !== last) {
    setLast(value)
    setDraft(value)
  }
  const commit = async () => {
    const next = draft.trim()
    if (next === value) return setError(undefined)
    const res = await parseRule(validate(next))
    if (!res.ok) return setError(res.error?.split("\n").at(-1))
    setError(undefined)
    onCommit(next)
  }
  // A highlighted copy of the text sits under a transparent input, the same
  // way the rule editor works, so values read like code while staying editable.
  return (
    <span className="grid self-stretch">
      <span
        aria-hidden
        className="pointer-events-none col-start-1 row-start-1 self-center justify-self-center px-2 text-center font-mono text-[13px] whitespace-pre"
      >
        {paint(draft, valueSegments(validate(draft), draft))}
      </span>
      <input
        aria-label={label}
        aria-invalid={!!error || undefined}
        title={error}
        value={draft}
        spellCheck={false}
        autoComplete="off"
        // Exactly as wide as the text (monospace) plus px-2 each side, so the
        // padding is equal left and right; `size` adds a browser-chosen extra.
        style={{ width: `calc(${Math.max(draft.length, 4)}ch + 1rem)` }}
        className="col-start-1 row-start-1 min-w-0 bg-transparent px-2 py-1 text-center font-mono text-[13px] text-transparent caret-foreground outline-none selection:bg-primary/30 focus:bg-accent/40 aria-invalid:bg-destructive/15 aria-invalid:underline aria-invalid:decoration-destructive aria-invalid:decoration-wavy"
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault()
            void commit()
          } else if (e.key === "Escape") {
            e.stopPropagation()
            setDraft(value)
            setError(undefined)
          }
        }}
      />
    </span>
  )
}

/** The field segment: a text box that suggests paths from the input JSON. */
function FieldInput({
  value,
  subtitle,
  onCommit,
}: {
  value: string
  /** Shown under the field (the preview's resolved value). */
  subtitle?: string
  onCommit(field: string): void
}) {
  const ctx = useCtx()
  const [draft, setDraft] = useState(value)
  const id = useId()
  const open = ctx.openList === id
  const setOpen = (next: boolean) => {
    if (next) ctx.setOpenList(id)
    else if (ctx.openList === id) ctx.setOpenList(undefined)
  }
  const [index, setIndex] = useState(0)
  const [last, setLast] = useState(value)
  if (value !== last) {
    setLast(value)
    setDraft(value)
  }
  // Untouched (just focused), list every field; typing narrows it.
  const query = draft === value ? "" : draft.toLowerCase()
  const matches = ctx.fields.filter((f) => f.path.toLowerCase().includes(query))
  const commit = (field: string) => {
    setOpen(false)
    const next = field.trim()
    if (!/^[A-Za-z_][\w.-]*$/.test(next)) return setDraft(value)
    if (next !== value) onCommit(next)
  }
  // While not previewing, hovering a field shows what the input has for it.
  const inputValue = ctx.preview
    ? undefined
    : (ctx.fields.find((f) => f.path === value)?.detail ?? "not in the input")
  const input = (
    <input
      aria-label="Field"
      value={draft}
      spellCheck={false}
      autoComplete="off"
      // Exactly as wide as the text (monospace) plus px-2 each side, so the
      // padding is equal left and right; `size` adds a browser-chosen extra.
      style={{ width: `calc(${Math.max(draft.length, 4)}ch + 1rem)` }}
      className="min-w-0 flex-1 bg-transparent px-2 py-1 text-center font-mono text-[13px] text-tok-id outline-none focus:bg-accent/40"
      onFocus={(e) => {
        e.target.select()
        setIndex(
          Math.max(
            0,
            ctx.fields.findIndex((f) => f.path === value)
          )
        )
        setOpen(true)
      }}
      onChange={(e) => {
        setDraft(e.target.value)
        setIndex(0)
        setOpen(true)
      }}
      onBlur={() => commit(draft)}
      onKeyDown={(e) => {
        if (
          open &&
          matches.length &&
          (e.key === "ArrowDown" || e.key === "ArrowUp")
        ) {
          e.preventDefault()
          setIndex(
            (i) =>
              (i + (e.key === "ArrowDown" ? 1 : -1) + matches.length) %
              matches.length
          )
        } else if (e.key === "Enter") {
          e.preventDefault()
          const pick = open && matches[index] ? matches[index].path : draft
          setDraft(pick)
          commit(pick)
        } else if (e.key === "Escape") {
          e.stopPropagation()
          setDraft(value)
          setOpen(false)
        }
      }}
    />
  )
  return (
    <span
      data-combobox
      className="relative flex flex-col self-stretch"
      // The whole segment, subtitle included, is the hit target.
      onMouseDown={(e) => {
        if (e.target instanceof HTMLInputElement) return
        e.preventDefault()
        e.currentTarget.querySelector("input")?.focus()
      }}
    >
      {inputValue && !open ? (
        <Tooltip>
          <TooltipTrigger render={input} />
          <TooltipContent
            side="top"
            align="start"
            className="bg-popover px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground ring-1 ring-foreground/10 [&>:last-child]:hidden"
          >
            {inputValue}
          </TooltipContent>
        </Tooltip>
      ) : (
        input
      )}
      {subtitle && (
        <span
          className={cn(
            "-mt-1 truncate px-2 pb-1 font-mono text-[10px] leading-3",
            subtitle === "missing" ? "text-missing" : "text-muted-foreground"
          )}
        >
          {subtitle}
        </span>
      )}
      {open && matches.length > 0 && (
        <ul
          role="listbox"
          className="absolute top-full left-0 z-20 mt-1 max-h-72 max-w-80 min-w-56 overflow-y-auto rounded-lg bg-popover p-1 text-popover-foreground shadow-lg ring-1 ring-foreground/10"
        >
          {matches.map((f, i) => (
            <li
              key={f.path}
              role="option"
              aria-selected={i === index}
              ref={
                i === index
                  ? (el) => el?.scrollIntoView({ block: "nearest" })
                  : undefined
              }
              className={cn(
                "flex cursor-default items-baseline gap-3 rounded-md px-2 py-1 font-mono text-[13px]",
                i === index && "bg-accent",
                f.path === value && "font-semibold"
              )}
              onMouseEnter={() => setIndex(i)}
              onMouseDown={(e) => {
                e.preventDefault()
                setDraft(f.path)
                commit(f.path)
              }}
            >
              <span className="truncate text-tok-id">{f.path}</span>
              <span className="ml-auto truncate font-sans text-xs text-muted-foreground">
                {f.detail}
              </span>
            </li>
          ))}
        </ul>
      )}
    </span>
  )
}

/**
 * The operator segment: type any operator spelling (`ne`, `matches`, …) or
 * pick one from the list that opens on focus.
 */
function OperatorInput({
  value,
  validate,
  onCommit,
}: {
  value: string
  /** The condition text this operator would produce, checked before committing. */
  validate(op: string): string
  onCommit(op: string): void
}) {
  const [draft, setDraft] = useState(value)
  const [last, setLast] = useState(value)
  const ctx = useCtx()
  const id = useId()
  const open = ctx.openList === id
  const setOpen = (next: boolean) => {
    if (next) ctx.setOpenList(id)
    else if (ctx.openList === id) ctx.setOpenList(undefined)
  }
  const [index, setIndex] = useState(0)
  const [error, setError] = useState<string>()
  if (value !== last) {
    setLast(value)
    setDraft(value)
  }
  // Untouched, the list shows every operator; typing narrows it to the
  // spellings that contain what you typed.
  const query =
    draft === value ? "" : draft.trim().toLowerCase().replace(/\s+/g, " ")
  const matches = OPERATOR_GROUPS.map((g) => ({
    ...g,
    shown: g.spellings.filter((sp) => sp.includes(query)),
  })).filter((g) => g.shown.length > 0)
  const current = matches.findIndex((g) => g.spellings.includes(value))
  const commit = async (op: string) => {
    setOpen(false)
    const next = op.trim().replace(/\s+/g, " ")
    if (!next || next === value) {
      setDraft(value)
      return setError(undefined)
    }
    const res = await parseRule(validate(next))
    if (!res.ok) {
      // Say why: often the operator is fine but the value doesn't suit it
      // (e.g. `matches` needs a /regex/).
      const lines = res.error?.split("\n") ?? []
      return setError(
        lines.find(
          (l) => l && !/^(syntax error|\s*\^)/.test(l) && !l.includes(next)
        ) ?? `“${next}” doesn’t work here`
      )
    }
    setError(undefined)
    onCommit(next)
  }
  return (
    <span data-combobox className="relative flex self-stretch border-x">
      <input
        aria-label="Operator"
        aria-invalid={!!error || undefined}
        title={error}
        value={draft}
        spellCheck={false}
        autoComplete="off"
        // Exactly as wide as the text (monospace) plus px-2 each side, so the
        // padding is equal left and right; `size` adds a browser-chosen extra.
        style={{ width: `calc(${Math.max(draft.length, 2)}ch + 1rem)` }}
        className="min-w-0 bg-transparent px-2 text-center font-mono text-[13px] text-tok-op outline-none focus:bg-accent/60 aria-invalid:bg-destructive/10 aria-invalid:text-destructive"
        onFocus={(e) => {
          e.target.select()
          setIndex(Math.max(0, current))
          setOpen(true)
        }}
        onChange={(e) => {
          setDraft(e.target.value)
          setIndex(0)
          setOpen(true)
          setError(undefined)
        }}
        onBlur={() => void commit(draft)}
        onKeyDown={(e) => {
          if (
            open &&
            matches.length &&
            (e.key === "ArrowDown" || e.key === "ArrowUp")
          ) {
            e.preventDefault()
            setIndex(
              (i) =>
                (i + (e.key === "ArrowDown" ? 1 : -1) + matches.length) %
                matches.length
            )
          } else if (e.key === "Enter") {
            e.preventDefault()
            // An exact spelling wins over the highlight, so `matches` isn't
            // replaced by the `not matches` it's a prefix of.
            const typed = draft.trim().toLowerCase().replace(/\s+/g, " ")
            const pick =
              SPELLINGS.has(typed) || !open || !matches[index]
                ? draft
                : matches[index].shown[0]
            setDraft(pick)
            void commit(pick)
          } else if (e.key === "Escape") {
            e.stopPropagation()
            setDraft(value)
            setOpen(false)
            setError(undefined)
          }
        }}
      />
      {open && matches.length > 0 && (
        <ul
          role="listbox"
          className="absolute top-full left-0 z-20 mt-1 w-max min-w-72 overflow-hidden rounded-lg bg-popover p-1 text-popover-foreground shadow-lg ring-1 ring-foreground/10"
        >
          {matches.map((g, i) => (
            <li
              key={g.spellings[0]}
              role="option"
              aria-selected={i === index}
              className={cn(
                "flex cursor-default items-baseline gap-3 rounded-md px-1 py-0.5",
                i === index && "bg-accent"
              )}
              onMouseEnter={() => setIndex(i)}
            >
              {/* Each spelling is clickable; the first is what Enter picks. */}
              <span className="flex shrink-0 gap-1 whitespace-nowrap">
                {g.shown.map((sp) => (
                  <button
                    key={sp}
                    type="button"
                    tabIndex={-1}
                    className={cn(
                      "rounded px-1 font-mono text-[13px] hover:bg-background/60",
                      sp === value
                        ? "font-semibold text-foreground"
                        : "text-tok-op"
                    )}
                    onMouseDown={(e) => {
                      e.preventDefault()
                      setDraft(sp)
                      void commit(sp)
                    }}
                  >
                    {sp}
                  </button>
                ))}
              </span>
              <span className="ml-auto pl-4 text-xs whitespace-nowrap text-muted-foreground">
                {g.meaning}
              </span>
            </li>
          ))}
        </ul>
      )}
    </span>
  )
}
