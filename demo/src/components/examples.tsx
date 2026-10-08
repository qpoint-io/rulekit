import { useEffect, useState } from "react"
import { TriangleAlertIcon } from "lucide-react"

import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import type { Playground } from "@/hooks/use-playground"
import { evalRule, type Status } from "@/lib/rulekit"
import { POLICIES, type Policy } from "@/lib/samples"
import { cn } from "@/lib/utils"

import { STATUS } from "./status"

/** The policy whose example was loaded last. */
const currentPolicy = (pg: Playground): Policy =>
  POLICIES.find((p) => p.name === pg.example.policy) ?? POLICIES[0]

/** Picks an example policy: loads its rule together with its first input. */
export function PolicyPicker({ pg }: { pg: Playground }) {
  const policy = currentPolicy(pg)
  const items = POLICIES.map((p) => ({ value: p.name, label: p.name }))
  return (
    <Select
      items={items}
      value={policy.name}
      onValueChange={(name) => {
        const next = POLICIES.find((p) => p.name === name)
        if (next)
          pg.load({ ...next.inputs[0], policy: next.name, rule: next.rule })
      }}
    >
      <SelectTrigger
        size="sm"
        aria-label="Example policy"
        className="h-7 gap-1 border-0 shadow-none dark:bg-transparent"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent className="min-w-64">
        <SelectGroup>
          {POLICIES.map((p) => (
            <SelectItem key={p.name} value={p.name}>
              <div className="flex flex-col">
                <span>{p.name}</span>
                <span className="text-xs text-muted-foreground">
                  {p.description}
                </span>
              </div>
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

/**
 * The current policy's inputs as test cases. Each chip swaps in its input
 * and shows the outcome it's meant to produce; a warning marks chips whose
 * live result under the current rule disagrees, so editing the rule shows
 * which cases it breaks.
 */
export function TestCaseStrip({ pg }: { pg: Playground }) {
  const policy = currentPolicy(pg)
  const [live, setLive] = useState<Record<string, Status | undefined>>({})

  useEffect(() => {
    if (!pg.ready) return
    let stale = false
    void Promise.all(
      policy.inputs.map(
        async (i) => [i.id, (await evalRule(pg.source, i.json)).status] as const
      )
    ).then((pairs) => {
      if (!stale) setLive(Object.fromEntries(pairs))
    })
    return () => {
      stale = true
    }
  }, [pg.ready, pg.source, policy])

  const custom = !policy.inputs.some((i) => i.json === pg.input)
  return (
    <div
      role="radiogroup"
      aria-label="Test inputs"
      className="flex flex-wrap gap-1.5"
    >
      {policy.inputs.map((input) => {
        const expected = STATUS[input.expect]
        const actual = live[input.id]
        const broken =
          pg.parsed?.ok && actual !== undefined && actual !== input.expect
        const active = input.json === pg.input
        return (
          <Tooltip key={input.id}>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  role="radio"
                  aria-checked={active}
                  onClick={() =>
                    pg.load({ ...input, policy: policy.name, rule: pg.source })
                  }
                  className={cn(
                    "flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/60",
                    active
                      ? "border-foreground/30 bg-accent text-foreground"
                      : "border-border text-muted-foreground hover:bg-accent/60 hover:text-foreground"
                  )}
                />
              }
            >
              <expected.icon className={cn("size-3.5", expected.tone)} />
              {input.label}
              {broken && (
                <TriangleAlertIcon
                  className="size-3.5 text-destructive"
                  aria-label="Result changed"
                />
              )}
            </TooltipTrigger>
            <TooltipContent className="max-w-xs">
              {input.description}. Expected {expected.label.toLowerCase()}
              {broken && actual
                ? `, now ${STATUS[actual].label.toLowerCase()}`
                : ""}
              .
            </TooltipContent>
          </Tooltip>
        )
      })}
      {custom && (
        <span className="flex h-7 items-center rounded-full border border-dashed border-foreground/30 bg-accent px-2.5 text-xs text-foreground">
          Custom input
        </span>
      )}
    </div>
  )
}
