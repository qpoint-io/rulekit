import type { AstNode } from "./rulekit"

/**
 * The rule as the Inline view's builder sees it: and/or groups (chains
 * flattened), negations, and conditions kept as their source text. Structural
 * edits happen here and the rule is re-printed, so they never splice text by
 * AST spans (which leave out a group's own parentheses). Comments are lost.
 */
export type RuleModel =
  | { kind: "group"; id: string; op: "and" | "or"; items: RuleModel[] }
  | { kind: "not"; id: string; item: RuleModel }
  | { kind: "cond"; id: string; text: string }

const isGroup = (node: AstNode) =>
  node.kind === "binary" && (node.operator === "and" || node.operator === "or")

export function fromAst(node: AstNode, grouped: Set<string>): RuleModel {
  if (isGroup(node)) {
    const items: RuleModel[] = []
    const gather = (n: AstNode) => {
      for (const child of n.children ?? []) {
        if (
          isGroup(child) &&
          child.operator === n.operator &&
          !grouped.has(child.id)
        )
          gather(child)
        else items.push(fromAst(child, grouped))
      }
    }
    gather(node)
    return {
      kind: "group",
      id: node.id,
      op: node.operator as "and" | "or",
      items,
    }
  }
  if (node.kind === "unary" && node.children?.length === 1) {
    return {
      kind: "not",
      id: node.id,
      item: fromAst(node.children[0], grouped),
    }
  }
  return { kind: "cond", id: node.id, text: node.text }
}

/** Rule text laid out like the multiline formatter: one operand per line, nested groups indented. */
export function print(model: RuleModel, depth = 0): string {
  const pad = "  ".repeat(depth)
  switch (model.kind) {
    case "cond":
      return model.text
    case "not":
      return model.item.kind === "cond"
        ? `not (${model.item.text})`
        : `not ${print(model.item, depth)}`
    case "group": {
      const inner = "  ".repeat(depth + 1)
      const lines = model.items.map((item) => print(item, depth + 1))
      if (depth === 0) {
        return model.items
          .map((item) => print(item, 1).replace(/^ {2}/gm, ""))
          .join(`\n${model.op} `)
      }
      return `(\n${inner}${lines.join(`\n${inner}${model.op} `)}\n${pad})`
    }
  }
}

/** Copy of `model` with the node `id` replaced by `fn(node)` (undefined removes it). */
export function update(
  model: RuleModel,
  id: string,
  fn: (node: RuleModel) => RuleModel | undefined
): RuleModel | undefined {
  if (model.id === id) return fn(model)
  if (model.kind === "not") {
    const item = update(model.item, id, fn)
    return item && { ...model, item }
  }
  if (model.kind === "group") {
    const items = model.items.flatMap((item) => update(item, id, fn) ?? [])
    // A group left with one operand is just that operand.
    if (items.length === 1) return items[0]
    if (items.length === 0) return undefined
    // Nested groups stay groups even when they share this operator, so an
    // and/or switch can always be switched back.
    return { ...model, items }
  }
  return model
}

/** Add a condition (already-valid rule text) at the end of group `id`, or alongside a lone root. */
export function addTo(model: RuleModel, id: string, text: string): RuleModel {
  // Conditions that are themselves and/or expressions keep their own parentheses.
  const cond: RuleModel = {
    kind: "cond",
    id: `new:${text}`,
    text: /\b(and|or)\b|&&|\|\|/i.test(text) ? `(${text})` : text,
  }
  if (model.id === id && model.kind !== "group") {
    return {
      kind: "group",
      id: `new-group:${id}`,
      op: "and",
      items: [model, cond],
    }
  }
  return update(model, id, (group) =>
    group.kind === "group" ? { ...group, items: [...group.items, cond] } : group
  )!
}

/** Find node `id`. */
export function find(model: RuleModel, id: string): RuleModel | undefined {
  if (model.id === id) return model
  if (model.kind === "not") return find(model.item, id)
  if (model.kind === "group") {
    for (const item of model.items) {
      const hit = find(item, id)
      if (hit) return hit
    }
  }
  return undefined
}

/** Insert `item` into group `groupId` at `index`. */
export function insertAt(
  model: RuleModel,
  groupId: string,
  index: number,
  item: RuleModel
): RuleModel {
  return update(model, groupId, (group) => {
    if (group.kind !== "group") return group
    const items = [...group.items]
    items.splice(index, 0, item)
    return { ...group, items }
  })!
}

/**
 * Move node `id` to position `index` of group `groupId` (an index into the
 * group as it is before the move). Dropping a group into itself is a no-op.
 */
export function move(
  model: RuleModel,
  id: string,
  groupId: string,
  index: number
): RuleModel {
  const item = find(model, id)
  const target = find(model, groupId)
  if (!item || !target || target.kind !== "group" || find(item, groupId))
    return model
  // Leave a placeholder so indices in the target group stay valid, insert, then drop it.
  const hole: RuleModel = { kind: "cond", id: "\u0000hole", text: "" }
  const swapped = update(model, id, () => hole)!
  const inserted = insertAt(swapped, groupId, index, item)
  return update(inserted, hole.id, () => undefined) ?? item
}
