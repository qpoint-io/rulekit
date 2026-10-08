import { useState } from "react"
import { CheckIcon, MousePointerClickIcon, Trash2Icon } from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldDescription,
  FieldError,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
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
import { deleteBlocker, editDraft, editKind, OPERATORS } from "@/lib/ast"
import type { AstNode } from "@/lib/rulekit"

import { kindLabel } from "./structure-tree"

type Props = {
  node?: AstNode
  byId: Map<string, AstNode>
  error?: string
  onRewrite(node: AstNode, replacement: string, kind: "operator" | "node"): void
  onDelete(node: AstNode): void
}

export function NodeInspector({ node, ...props }: Props) {
  if (!node) {
    return (
      <div className="flex items-center gap-2 px-1 text-sm text-muted-foreground">
        <MousePointerClickIcon className="size-4 shrink-0" />
        Select a step, or put the cursor in the rule, to edit it.
      </div>
    )
  }
  // Remount when the node changes so the draft starts from its current value.
  return <Editor key={`${node.id}:${node.text}`} node={node} {...props} />
}

function Editor({
  node,
  byId,
  error,
  onRewrite,
  onDelete,
}: Props & { node: AstNode }) {
  const kind = editKind(node)
  const [draft, setDraft] = useState(() => editDraft(node))
  const blocker = deleteBlocker(node, byId)
  const unchanged = draft === editDraft(node)
  const inputId = `edit-${node.id}`

  const deleteButton = (
    <Button
      variant="ghost"
      size="icon"
      aria-label="Delete node"
      disabled={!!blocker}
      onClick={() => onDelete(node)}
    >
      <Trash2Icon />
    </Button>
  )

  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault()
        if (!kind || unchanged) return
        onRewrite(
          node,
          kind === "path" ? draft.trim() : draft,
          kind === "operator" ? "operator" : "node"
        )
      }}
    >
      <Field data-invalid={!!error || undefined}>
        <div className="flex items-center gap-2">
          <Badge variant="secondary">{kindLabel(node)}</Badge>
          <FieldLabel htmlFor={inputId} className="min-w-0 flex-1">
            <Tooltip>
              <TooltipTrigger
                render={
                  <span className="truncate font-mono text-xs font-normal text-muted-foreground" />
                }
              >
                {node.text}
              </TooltipTrigger>
              <TooltipContent className="max-w-sm font-mono break-words">
                {node.text}
              </TooltipContent>
            </Tooltip>
          </FieldLabel>
        </div>
        <div className="flex items-center gap-2">
          {kind === "operator" ? (
            <Select
              items={OPERATORS}
              value={draft}
              onValueChange={(value) => value && setDraft(value)}
            >
              <SelectTrigger
                id={inputId}
                className="w-40 font-mono"
                aria-invalid={!!error || undefined}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {OPERATORS.map((op) => (
                    <SelectItem
                      key={op.value}
                      value={op.value}
                      className="font-mono"
                    >
                      {op.label}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          ) : kind ? (
            <Input
              id={inputId}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              className="flex-1 font-mono"
              spellCheck={false}
              autoComplete="off"
              aria-invalid={!!error || undefined}
            />
          ) : (
            <FieldDescription className="flex-1">
              {node.kind === "array"
                ? "Edit list items individually."
                : "Edit the expression inside."}
            </FieldDescription>
          )}
          {kind && (
            <Button type="submit" variant="outline" disabled={unchanged}>
              <CheckIcon data-icon="inline-start" />
              Apply
            </Button>
          )}
          {blocker ? (
            <Tooltip>
              <TooltipTrigger
                render={<span className="ml-auto" tabIndex={0} />}
              >
                {deleteButton}
              </TooltipTrigger>
              <TooltipContent>{blocker}</TooltipContent>
            </Tooltip>
          ) : (
            <span className="ml-auto">{deleteButton}</span>
          )}
        </div>
        {error && <FieldError>{error}</FieldError>}
      </Field>
    </form>
  )
}
