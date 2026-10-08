import type { EvalResponse } from "@/lib/rulekit"
import { cn } from "@/lib/utils"

import { STATUS } from "./status"

export function Verdict({ result }: { result: EvalResponse }) {
  const status = STATUS[result.status ?? "unknown"]
  return (
    <div className="flex flex-col gap-3">
      <div role="status" className="flex items-center gap-3">
        <status.icon className={cn("size-7", status.tone)} strokeWidth={2.25} />
        <span
          className={cn("text-2xl font-semibold tracking-tight", status.tone)}
        >
          {status.verdict}
        </span>
        {result.value !== undefined && (
          <span className="ml-auto truncate font-mono text-sm text-muted-foreground">
            returned{" "}
            <span className="text-foreground">
              {JSON.stringify(result.value)}
            </span>
          </span>
        )}
      </div>
      {result.error && (
        <p className="font-mono text-sm break-words text-destructive">
          {result.error}
        </p>
      )}
      {result.missingFields?.length ? (
        <p className="text-sm text-muted-foreground">
          Add{" "}
          {result.missingFields.map((field, i) => (
            <span key={field}>
              {i > 0 && ", "}
              <code className="rounded bg-missing/15 px-1 py-0.5 font-mono text-[12px] text-missing">
                {field}
              </code>
            </span>
          ))}{" "}
          to the input to get a definite result.
        </p>
      ) : null}
    </div>
  )
}
