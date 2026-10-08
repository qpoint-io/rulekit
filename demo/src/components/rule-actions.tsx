import { useEffect, useState, type ReactNode } from "react"
import {
  CheckIcon,
  ChevronDownIcon,
  CircleHelpIcon,
  CopyIcon,
  FileJsonIcon,
  LinkIcon,
  Redo2Icon,
  RotateCcwIcon,
  Undo2Icon,
  WandSparklesIcon,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import type { Playground } from "@/hooks/use-playground"
import { lexRule } from "@/lib/highlight"
import { POLICIES } from "@/lib/samples"
import { cn } from "@/lib/utils"

import { paint } from "./code-editor"
import { STATUS } from "./status"

const MOD =
  typeof navigator !== "undefined" && /Mac|iP/.test(navigator.platform)
    ? "⌘"
    : "Ctrl+"

function IconAction({
  label,
  shortcut,
  onClick,
  disabled,
  children,
}: {
  label: string
  shortcut?: string
  onClick(): void
  disabled?: boolean
  children: ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={label}
            onClick={onClick}
            disabled={disabled}
          />
        }
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>
        {label}
        {shortcut && (
          <span className="ml-2 text-muted-foreground">{shortcut}</span>
        )}
      </TooltipContent>
    </Tooltip>
  )
}

/** Undo/redo, copy and share, format, reset, and a syntax cheat sheet for the rule editor. */
export function RuleActions({ pg }: { pg: Playground }) {
  const [copied, setCopied] = useState<"rule" | "link">()
  useEffect(() => {
    if (!copied) return
    const t = setTimeout(() => setCopied(undefined), 1500)
    return () => clearTimeout(t)
  }, [copied])

  const copy = (what: "rule" | "link") => {
    void navigator.clipboard.writeText(
      what === "rule" ? pg.source : pg.shareUrl()
    )
    setCopied(what)
  }

  return (
    <div className="flex items-center gap-0.5">
      <IconAction
        label="Undo"
        shortcut={`${MOD}Z`}
        onClick={pg.undo}
        disabled={!pg.canUndo}
      >
        <Undo2Icon />
      </IconAction>
      <IconAction
        label="Redo"
        shortcut={`${MOD}⇧Z`}
        onClick={pg.redo}
        disabled={!pg.canRedo}
      >
        <Redo2Icon />
      </IconAction>
      <IconAction
        label={copied === "rule" ? "Copied" : "Copy rule"}
        onClick={() => copy("rule")}
      >
        {copied === "rule" ? <CheckIcon /> : <CopyIcon />}
      </IconAction>
      <IconAction
        label={
          copied === "link" ? "Link copied" : "Copy link to this rule and input"
        }
        onClick={() => copy("link")}
      >
        {copied === "link" ? <CheckIcon /> : <LinkIcon />}
      </IconAction>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button variant="ghost" size="sm" disabled={!pg.parsed?.ok} />
          }
        >
          <WandSparklesIcon data-icon="inline-start" />
          Format
          <ChevronDownIcon data-icon="inline-end" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuGroup>
            <DropdownMenuItem onClick={() => pg.format("multiline")}>
              Multiline
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => pg.format("compact")}>
              One line
            </DropdownMenuItem>
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      <DropdownMenu>
        <DropdownMenuTrigger render={<Button variant="ghost" size="sm" />}>
          <FileJsonIcon data-icon="inline-start" />
          Examples
          <ChevronDownIcon data-icon="inline-end" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-96">
          {POLICIES.map((policy, i) => {
            // The rule without its comments: the description already says what it's for.
            const rulePreview = policy.rule.replace(/^\s*--.*\n/gm, "")
            return (
              <DropdownMenuGroup key={policy.name}>
                {i > 0 && <DropdownMenuSeparator />}
                {/* The policy first: what it's for and the rule itself. */}
                <div className="flex flex-col gap-1 px-1.5 pt-1.5 pb-1">
                  <span className="text-sm font-medium text-foreground">
                    {policy.name}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    {policy.description}
                  </span>
                  <pre className="mt-0.5 rounded-md bg-background px-2 py-1.5 font-mono text-[10px] leading-[14px] whitespace-pre-wrap text-muted-foreground ring-1 ring-border">
                    {paint(rulePreview, lexRule(rulePreview))}
                  </pre>
                </div>
                {policy.inputs.map((input) => {
                  const outcome = STATUS[input.expect]
                  return (
                    <DropdownMenuItem
                      key={input.id}
                      className={cn(
                        "items-start focus:**:[.text-fail]:text-fail focus:**:[.text-missing]:text-missing focus:**:[.text-muted-foreground]:text-muted-foreground focus:**:[.text-pass]:text-pass focus:*:[svg.text-fail]:text-fail focus:*:[svg.text-missing]:text-missing focus:*:[svg.text-pass]:text-pass",
                        // The loaded input, marked since the menu stays open.
                        pg.example.id === input.id &&
                          pg.source === policy.rule &&
                          pg.input === input.json &&
                          "bg-accent/60"
                      )}
                      // Stay open so inputs can be compared one after another.
                      closeOnClick={false}
                      onClick={() =>
                        pg.load({
                          ...input,
                          policy: policy.name,
                          rule: policy.rule,
                        })
                      }
                    >
                      <outcome.icon className={cn("mt-0.5", outcome.tone)} />
                      <div className="flex min-w-0 flex-1 flex-col">
                        <span className="flex items-baseline gap-2">
                          {input.label}
                          <span
                            className={cn(
                              "ml-auto text-xs font-medium",
                              outcome.tone
                            )}
                          >
                            {outcome.verdict}
                          </span>
                        </span>
                        <span className="text-xs text-muted-foreground">
                          {input.description}
                        </span>
                      </div>
                    </DropdownMenuItem>
                  )
                })}
              </DropdownMenuGroup>
            )
          })}
        </DropdownMenuContent>
      </DropdownMenu>
      <IconAction label={`Reset to “${pg.example.label}”`} onClick={pg.reset}>
        <RotateCcwIcon />
      </IconAction>
      <Popover>
        <Tooltip>
          <TooltipTrigger
            render={
              <PopoverTrigger
                render={
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label="Syntax reference"
                  />
                }
              />
            }
          >
            <CircleHelpIcon />
          </TooltipTrigger>
          <TooltipContent>Syntax reference</TooltipContent>
        </Tooltip>
        <PopoverContent align="end" className="w-96">
          <SyntaxReference />
        </PopoverContent>
      </Popover>
    </div>
  )
}

const SYNTAX: [string, string][] = [
  ["dst.port == 443", "equals; also != > >= < <="],
  ["dst.port ne 443", "word forms: eq ne gt ge lt le"],
  ['host not contains "x"', "not in, not contains, not matches"],
  ["dst.port in [80, 443]", "one of a list"],
  ["dst.ip in 10.0.0.0/8", "inside a CIDR range"],
  ['host contains "api"', "substring or list member"],
  ["path =~ /^\\/v[0-9]+/", "matches a regex; also matches"],
  ["tls.enabled == true", "booleans"],
  ["a and b, a or b", "also && and ||"],
  ["not (a or b)", "negation and grouping"],
  ["-- note, /* note */", "comments"],
]

function SyntaxReference() {
  return (
    <div className="flex flex-col gap-2">
      <p className="text-sm font-medium">Rule syntax</p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-xs">
        {SYNTAX.map(([code, meaning]) => (
          <div key={code} className="contents">
            <dt className="font-mono text-foreground">{code}</dt>
            <dd className="text-muted-foreground">{meaning}</dd>
          </div>
        ))}
      </dl>
      <p className="text-xs text-muted-foreground">
        Fields are dotted paths into the input JSON. Press Ctrl+Space in the
        editor for suggestions.
      </p>
    </div>
  )
}
