import { useEffect, useMemo, useState, useSyncExternalStore } from "react"
import {
  BoxesIcon,
  SlidersHorizontalIcon,
  ListTreeIcon,
  NetworkIcon,
} from "lucide-react"
import {
  CheckIcon,
  ScrollTextIcon,
  TriangleAlertIcon,
  Undo2Icon,
} from "lucide-react"

import { CodeEditor, type Mark } from "@/components/code-editor"
import { LinkLines } from "@/components/link-lines"
import { NodeInspector } from "@/components/node-inspector"
import { PolicyPicker, TestCaseStrip } from "@/components/examples"
import { RuleActions } from "@/components/rule-actions"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { useDefaultLayout } from "react-resizable-panels"
import { RuleEditor } from "@/components/rule-editor"
import { RuleBuilder } from "@/components/rule-builder"
import { STATUS } from "@/components/status"
import {
  FitWidth,
  StructureBlocks,
  StructureGraph,
  StructureTree,
} from "@/components/structure-tree"
import { Verdict } from "@/components/verdict"
import { ThemeToggle } from "@/components/theme-toggle"
import {
  Alert,
  AlertAction,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { usePlayground, type Playground } from "@/hooks/use-playground"
import { parseSyntaxError } from "@/lib/ast"
import { inputFields } from "@/lib/complete"
import { findJsonKey, lexJson } from "@/lib/highlight"
import { cn } from "@/lib/utils"

// Cards fill their grid cell on wide screens and get a fixed height when stacked.
const PANEL = "min-h-0 h-[28rem] lg:h-auto"

const VIEWS = [
  { value: "builder", label: "Builder", icon: SlidersHorizontalIcon },
  { value: "graph", label: "Graph", icon: NetworkIcon },
  { value: "blocks", label: "Blocks", icon: BoxesIcon },
  { value: "tree", label: "Tree", icon: ListTreeIcon },
] as const
type View = (typeof VIEWS)[number]["value"]

// Clicks inside these keep the current selection: controls, menus and the
// node editor (the rule editor selects from its caret instead).
const KEEPS_SELECTION =
  'button, a, input, textarea, select, [role="option"], [role="menu"], [role="menuitem"], [role="menuitemradio"], [role="listbox"], [data-slot="card-footer"], [data-keep-selection]'

export default function App() {
  const pg = usePlayground()
  const wide = useMediaQuery("(min-width: 1024px)")
  const columns = useDefaultLayout({
    id: "rulekit-columns",
    storage: localStorage,
  })
  const rows = useDefaultLayout({ id: "rulekit-rows", storage: localStorage })
  const [view, setView] = useState<View>("builder")
  // The builder hides results unless asked, so it stays about building.
  const [preview, setPreview] = useState(false)
  const building = view === "builder"
  const showResults = !building || preview
  const viewProps = {
    selectedId: pg.selected?.id,
    hoveredId: pg.hovered?.id,
    traceBySpan: pg.traceBySpan,
    grouped: pg.grouped,
    onSelect: pg.toggle,
    onHover: pg.hover,
  }
  const { select, undo, redo } = pg
  // Escape, or a click on anything that isn't a control, clears the selection.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") select()
      // Undo/redo cover every rule change, including ones made from the
      // views; the JSON editor and other fields keep their native undo.
      const target = e.target as Element
      const inRule = target.closest('textarea[aria-label="Rule source"]')
      if (
        (e.metaKey || e.ctrlKey) &&
        e.key.toLowerCase() === "z" &&
        (inRule || !target.closest("input, textarea"))
      ) {
        e.preventDefault()
        if (e.shiftKey) redo()
        else undo()
      }
    }
    const onClick = (e: MouseEvent) => {
      const target = e.target as Element
      if (!target.closest(KEEPS_SELECTION)) select()
    }
    window.addEventListener("keydown", onKey)
    document.addEventListener("click", onClick)
    return () => {
      window.removeEventListener("keydown", onKey)
      document.removeEventListener("click", onClick)
    }
  }, [select, undo, redo])
  const inputSegments = useMemo(() => lexJson(pg.input), [pg.input])
  const fields = useMemo(() => inputFields(pg.input), [pg.input])
  const jsonError = useMemo(() => {
    try {
      JSON.parse(pg.input)
      return undefined
    } catch (err) {
      return err instanceof Error ? err.message : String(err)
    }
  }, [pg.input])
  const hovering = !!pg.hovered && pg.hovered !== pg.selected
  const active = pg.hovered ?? pg.selected
  const selectedId = pg.selected?.id
  const rootId = pg.shown?.ast?.id

  // A field node points at a key in the input; mark it so it can be linked.
  const inputKey =
    active?.kind === "path" && active.path
      ? findJsonKey(pg.input, active.path.split("."))
      : undefined
  const inputMarks: Mark[] = inputKey
    ? [
        {
          ...inputKey,
          className: hovering ? "bg-foreground/[0.06]" : "bg-selection/15",
          link: "input",
        },
      ]
    : []

  // Keep the selected node visible; with no selection, start the graph on the root.
  useEffect(() => {
    const id = selectedId ?? (view === "graph" ? rootId : undefined)
    if (!id) return
    document
      .querySelector(`[data-node-id="${CSS.escape(id)}"]`)
      ?.scrollIntoView({
        block: "nearest",
        inline: selectedId ? "nearest" : "center",
      })
  }, [selectedId, rootId, view])

  const parseError = pg.parsed && !pg.parsed.ok ? pg.parsed.error : undefined
  const syntaxError = parseError ? parseSyntaxError(parseError) : undefined
  const status =
    pg.result && !jsonError ? STATUS[pg.result.status ?? "unknown"] : undefined

  const ruleCard = (
    <Card className={cn(PANEL, "lg:h-full")}>
      <CardHeader>
        <CardTitle className="flex items-center gap-1">
          Rule
          <span className="text-muted-foreground">·</span>
          <PolicyPicker pg={pg} />
        </CardTitle>
        <CardDescription>
          Type a rule; input fields autocomplete.
        </CardDescription>
        <CardAction>
          <RuleActions pg={pg} />
        </CardAction>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-3">
        <RuleEditor
          fields={fields}
          source={pg.source}
          onChange={pg.setSource}
          tokens={pg.parsed?.ok ? pg.parsed.tokens : undefined}
          hoverSpan={
            !pg.stale && pg.hovered && pg.hovered !== pg.selected
              ? pg.hovered.span
              : undefined
          }
          // Spans belong to the last good parse; they'd point at the wrong text now.
          selectedSpan={pg.stale ? undefined : pg.selected?.span}
          syntaxError={syntaxError}
          onCaret={pg.selectAt}
        />
        {pg.source.trim() === "" ? (
          <p className="text-sm text-muted-foreground">
            Start with a comparison like{" "}
            <code className="font-mono text-foreground">dst.port == 443</code>,
            or pick a field from the suggestions.
          </p>
        ) : (
          parseError && (
            <Alert variant="destructive">
              <TriangleAlertIcon />
              <AlertTitle>
                {syntaxError
                  ? `Syntax error on line ${syntaxError.line}, column ${syntaxError.column}`
                  : "Syntax error"}
              </AlertTitle>
              <AlertDescription className="font-mono text-xs">
                {syntaxError?.reason ?? parseError}
              </AlertDescription>
            </Alert>
          )
        )}
      </CardContent>
    </Card>
  )
  const inputCard = (
    <Card className={cn(PANEL, "lg:h-full")}>
      <CardHeader>
        <CardTitle>Input</CardTitle>
        <CardDescription>
          The JSON event the rule is checked against.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-3">
        <TestCaseStrip pg={pg} />
        <CodeEditor
          label="Input JSON"
          value={pg.input}
          onChange={pg.setInput}
          segments={inputSegments}
          marks={inputMarks}
        />
        {jsonError && (
          <Alert variant="destructive">
            <TriangleAlertIcon />
            <AlertTitle>The input isn’t valid JSON</AlertTitle>
            <AlertDescription className="font-mono text-xs">
              {jsonError}
            </AlertDescription>
          </Alert>
        )}
      </CardContent>
    </Card>
  )
  const evalCard = (
    <Card className="h-[40rem] min-h-0 lg:h-full">
      <CardHeader>
        <CardTitle>Evaluation</CardTitle>
        <CardDescription>
          Each step of the rule and what it evaluated to. Select a step to edit
          it.
        </CardDescription>
        <ul
          aria-label="Legend"
          className="flex flex-wrap gap-x-3 gap-y-1 pt-1 text-xs text-muted-foreground"
        >
          {(["passed", "failed", "missing", "error", "pruned"] as const).map(
            (key) => {
              const { icon: Icon, label, tone } = STATUS[key]
              return (
                <li key={key} className="flex items-center gap-1">
                  <Icon className={cn("size-3.5", tone)} />
                  {key === "passed"
                    ? "True"
                    : key === "failed"
                      ? "False"
                      : label}
                </li>
              )
            }
          )}
        </ul>
        <CardAction>
          <ToggleGroup
            size="sm"
            variant="outline"
            spacing={0}
            value={[view]}
            onValueChange={(next) => next[0] && setView(next[0] as View)}
          >
            {VIEWS.map(({ value, label, icon: Icon }) => (
              <Tooltip key={value}>
                <TooltipTrigger
                  render={<ToggleGroupItem value={value} aria-label={label} />}
                >
                  <Icon />
                </TooltipTrigger>
                <TooltipContent>{label}</TooltipContent>
              </Tooltip>
            ))}
          </ToggleGroup>
        </CardAction>
        {/* Always laid out so switching views or toggling never moves the rule. */}
        <div className="col-span-full flex items-center gap-2 pt-1 text-xs text-muted-foreground">
          <label
            className={cn(
              "flex w-fit items-center gap-2",
              !building && "invisible"
            )}
            aria-hidden={!building || undefined}
          >
            <Switch
              size="sm"
              checked={preview}
              onCheckedChange={setPreview}
              disabled={!building}
            />
            Preview results in the builder
          </label>
          <CopyLogButton pg={pg} />
        </div>
      </CardHeader>
      <CardContent className="relative">
        {jsonError ? (
          <p className="text-sm text-muted-foreground">
            Fix the input JSON to see a result.
          </p>
        ) : (
          pg.result && (
            <div
              className={cn(!showResults && "invisible")}
              aria-hidden={!showResults || undefined}
            >
              <Verdict result={pg.result} />
            </div>
          )
        )}
        {!showResults && !jsonError && (
          <p className="absolute inset-0 flex items-center px-(--card-spacing) text-sm text-muted-foreground">
            Result hidden while building. Turn on preview to see it.
          </p>
        )}
      </CardContent>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-2 border-t px-2 pt-2">
        {pg.stale && (
          <Alert className="mx-1 w-auto">
            <TriangleAlertIcon />
            <AlertTitle>Showing the last version that parsed</AlertTitle>
            <AlertDescription>
              {view === "builder"
                ? "The rule text has a syntax error. Editing here replaces the text with this version."
                : "The rule text has a syntax error. Fix it in the editor to update this view."}
            </AlertDescription>
          </Alert>
        )}
        {!pg.shown?.ast ? (
          <WaitingForRule
            ready={pg.ready}
            error={syntaxError ?? parseError}
            canUndo={pg.canUndo}
            onUndo={pg.undo}
            onShow={() => {
              const el = document.querySelector<HTMLTextAreaElement>(
                'textarea[aria-label="Rule source"]'
              )
              if (!el) return
              // Put the caret at the error so it's one keystroke from fixed.
              const lines = pg.source.split("\n")
              const at = syntaxError
                ? lines
                    .slice(0, syntaxError.line - 1)
                    .reduce((n, l) => n + l.length + 1, 0) +
                  syntaxError.column -
                  1
                : 0
              el.focus()
              el.setSelectionRange(at, at)
            }}
          />
        ) : view === "tree" ? (
          <ScrollArea className="min-h-0 flex-1">
            <ul role="tree" aria-label="Rule structure" className="pr-2">
              <StructureTree node={pg.shown.ast} {...viewProps} />
            </ul>
          </ScrollArea>
        ) : view === "graph" ? (
          <FitWidth>
            <StructureGraph node={pg.shown.ast} {...viewProps} />
          </FitWidth>
        ) : view === "builder" ? (
          <ScrollArea className="min-h-0 flex-1">
            <RuleBuilder
              node={pg.shown.ast}
              {...viewProps}
              preview={preview}
              byId={pg.byId}
              fields={fields}
              onReplace={pg.replace}
            />
          </ScrollArea>
        ) : (
          <ScrollArea className="min-h-0 flex-1">
            <div className="p-1 pr-3">
              <StructureBlocks node={pg.shown.ast} {...viewProps} />
            </div>
          </ScrollArea>
        )}
      </CardContent>
      {/* The builder edits in place, so it skips the editor bar. */}
      {pg.shown?.ast && !pg.stale && view !== "builder" && (
        <CardFooter className="border-t">
          <div className="w-full">
            <NodeInspector
              node={pg.selected}
              byId={pg.byId}
              error={pg.editError}
              onRewrite={pg.rewrite}
              onDelete={pg.remove}
            />
          </div>
        </CardFooter>
      )}
    </Card>
  )

  return (
    <div className="flex min-h-dvh flex-col lg:h-dvh">
      <header className="flex h-14 shrink-0 items-center gap-3 border-b px-4">
        <span
          aria-hidden
          className="grid size-7 place-items-center rounded-md bg-primary font-mono text-sm font-bold text-primary-foreground"
        >
          rk
        </span>
        <h1 className="text-[15px] font-semibold tracking-tight">rulekit</h1>
        <span className="text-sm text-muted-foreground">playground</span>
        <div className="ml-auto flex items-center gap-2">
          {status && (
            <Badge
              variant="outline"
              className="h-6 gap-1.5 px-2.5"
              aria-live="polite"
            >
              <status.icon className={status.tone} />
              {status.verdict}
            </Badge>
          )}
          <ThemeToggle />
        </div>
      </header>
      {pg.crash && (
        <Alert variant="destructive" className="mx-3 mt-3 w-auto">
          <TriangleAlertIcon />
          <AlertTitle>The rule engine crashed</AlertTitle>
          <AlertDescription className="flex flex-col gap-1">
            <span className="font-mono text-xs break-words">{pg.crash}</span>
            <span>
              This is a bug in rulekit, not your rule. Copy the change log to
              report it; reload if the playground stops responding.
            </span>
          </AlertDescription>
          <AlertAction>
            <Button variant="ghost" size="xs" onClick={pg.dismissCrash}>
              Dismiss
            </Button>
          </AlertAction>
        </Alert>
      )}

      {pg.loadError && (
        <Alert variant="destructive" className="mx-3 mt-3 w-auto">
          <TriangleAlertIcon />
          <AlertTitle>The rule engine didn’t load</AlertTitle>
          <AlertDescription>
            {pg.loadError}. Rebuild it with npm run wasm, then reload.
          </AlertDescription>
        </Alert>
      )}

      <main className="flex flex-1 flex-col gap-3 p-3 lg:min-h-0">
        {wide ? (
          // Drag the gaps between panels to resize; sizes are remembered.
          <ResizablePanelGroup
            orientation="horizontal"
            className="min-h-0 flex-1"
            {...columns}
          >
            <ResizablePanel id="authoring" defaultSize="50" minSize="25">
              <ResizablePanelGroup orientation="vertical" {...rows}>
                <ResizablePanel id="rule" defaultSize="50" minSize="15">
                  {ruleCard}
                </ResizablePanel>
                <ResizableHandle withHandle className="my-1.5 bg-transparent" />
                <ResizablePanel id="input" defaultSize="50" minSize="15">
                  {inputCard}
                </ResizablePanel>
              </ResizablePanelGroup>
            </ResizablePanel>
            <ResizableHandle withHandle className="mx-1.5 bg-transparent" />
            <ResizablePanel id="evaluation" defaultSize="50" minSize="25">
              {evalCard}
            </ResizablePanel>
          </ResizablePanelGroup>
        ) : (
          <>
            {ruleCard}
            {inputCard}
            {evalCard}
          </>
        )}
      </main>
      <LinkLines
        chain={
          active && !pg.stale
            ? [
                `mark[data-link="${hovering ? "hover" : "selected"}"]`,
                // The view may only render an ancestor (the Builder shows whole
                // conditions), so fall back from the node to its parents.
                active.id
                  .split(".")
                  .map((_, i, parts) =>
                    parts.slice(0, parts.length - i).join(".")
                  )
                  .map((id) => `[data-node-id="${CSS.escape(id)}"]`),
                'mark[data-link="input"]',
              ]
            : []
        }
        tone={hovering ? "hover" : "selected"}
      />
    </div>
  )
}

/**
 * Shown in place of a view while the rule doesn't parse: what's wrong, and
 * the two ways out (undo the change, or jump to it in the editor).
 */
function WaitingForRule({
  ready,
  error,
  canUndo,
  onUndo,
  onShow,
}: {
  ready: boolean
  error?: { line: number; column: number; reason: string } | string
  canUndo?: boolean
  onUndo?(): void
  onShow?(): void
}) {
  if (!ready) {
    return (
      <Empty className="flex-1">
        <EmptyHeader>
          <EmptyTitle>Loading the rule engine</EmptyTitle>
          <EmptyDescription>
            This takes a moment on first load.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }
  const where =
    typeof error === "object"
      ? `Line ${error.line}, column ${error.column}: `
      : ""
  const reason = typeof error === "object" ? error.reason : error
  return (
    <Empty className="flex-1">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <TriangleAlertIcon />
        </EmptyMedia>
        <EmptyTitle>The rule doesn’t parse</EmptyTitle>
        <EmptyDescription>
          {where}
          <span className="font-mono">{reason ?? "syntax error"}</span>
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent className="flex-row justify-center gap-2">
        {canUndo && (
          <Button variant="outline" size="sm" onClick={onUndo}>
            <Undo2Icon data-icon="inline-start" />
            Undo last change
          </Button>
        )}
        <Button variant="ghost" size="sm" onClick={onShow}>
          Show in editor
        </Button>
      </EmptyContent>
    </Empty>
  )
}

/**
 * Copies every change made this session (builder actions, edits, undo,
 * examples) with the rule each one produced, so a state can be reported
 * without retracing the steps by hand.
 */
function CopyLogButton({ pg }: { pg: Playground }) {
  const [copied, setCopied] = useState(false)
  useEffect(() => {
    if (!copied) return
    const t = setTimeout(() => setCopied(false), 1500)
    return () => clearTimeout(t)
  }, [copied])
  const steps = pg.log.length - 1
  return (
    <span className="ml-auto flex items-center gap-1">
      <Tooltip>
        <TooltipTrigger
          render={
            <Button
              variant="ghost"
              size="xs"
              onClick={() => {
                void navigator.clipboard.writeText(pg.logText())
                setCopied(true)
              }}
            />
          }
        >
          {copied ? (
            <CheckIcon data-icon="inline-start" />
          ) : (
            <ScrollTextIcon data-icon="inline-start" />
          )}
          {copied ? "Log copied" : `Copy change log (${steps})`}
        </TooltipTrigger>
        <TooltipContent>
          Every change this session and the rule after each, as Markdown
        </TooltipContent>
      </Tooltip>
      {steps > 0 && (
        <Button variant="ghost" size="xs" onClick={pg.clearLog}>
          Clear
        </Button>
      )}
    </span>
  )
}

/** Whether `query` matches, kept in sync as the window changes. */
function useMediaQuery(query: string) {
  return useSyncExternalStore(
    (onChange) => {
      const list = window.matchMedia(query)
      list.addEventListener("change", onChange)
      return () => list.removeEventListener("change", onChange)
    },
    () => window.matchMedia(query).matches
  )
}
