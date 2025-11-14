import type { ASTNode, Context, EvalResult } from "./types.js";
import { compare } from "./operators.js";
import { getNestedValue, isZero } from "./utils.js";
import { stringify } from "./stringify.js";

/**
 * Evaluates an AST node against a context
 */
export function evaluate(node: ASTNode, ctx: Context): EvalResult {
  switch (node.node_type) {
    case "operator":
      return evaluateOperator(node, ctx);
    case "field":
      return evaluateField(node, ctx);
    case "literal":
      return evaluateLiteral(node, ctx);
    case "array":
      return evaluateArray(node, ctx);
    case "function":
      return evaluateFunction(node, ctx);
    default:
      return { ok: false, value: null, error: `Unknown node type` };
  }
}

function evaluateOperator(node: ASTNode & { node_type: "operator" }, ctx: Context): EvalResult {
  const { operator, left, right } = node;

  // Handle unary operators
  if (operator === "not") {
    if (!right) {
      return { ok: false, value: null, error: "not operator requires right operand" };
    }
    const result = evaluate(right, ctx);
    if (!result.ok) return result;
    return { ok: true, value: !result.value };
  }

  // Binary operators require both operands
  if (!left || !right) {
    return { ok: false, value: null, error: `${operator} requires two operands` };
  }

  // Short-circuit evaluation for logical operators
  if (operator === "and") {
    const leftResult = evaluate(left, ctx);
    // Short circuit if left fails (is false/zero) or has error
    if (!leftResult.ok || isZero(leftResult.value)) {
      return leftResult.ok ? { ok: true, value: false } : leftResult;
    }
    
    const rightResult = evaluate(right, ctx);
    // Short circuit if right fails (is false/zero) or has error
    if (!rightResult.ok || isZero(rightResult.value)) {
      return rightResult.ok ? { ok: true, value: false } : rightResult;
    }
    
    return { ok: true, value: true };
  }

  if (operator === "or") {
    const leftResult = evaluate(left, ctx);
    if (leftResult.ok && !isZero(leftResult.value)) {
      return { ok: true, value: true };
    }
    
    const rightResult = evaluate(right, ctx);
    if (rightResult.ok && !isZero(rightResult.value)) {
      return { ok: true, value: true };
    }
    
    // If only one is not ok, return it
    if (leftResult.ok && !rightResult.ok) {
      return rightResult;
    } else if (!leftResult.ok && rightResult.ok) {
      return leftResult;
    }
    
    // Both are ok or both have errors
    if (leftResult.ok && rightResult.ok) {
      return { ok: true, value: !isZero(leftResult.value) || !isZero(rightResult.value) };
    }
    
    // Both have errors - return left error (or could coalesce)
    return leftResult;
  }

  // Evaluate both sides for comparison operators
  const leftResult = evaluate(left, ctx);
  if (!leftResult.ok) return leftResult;

  const rightResult = evaluate(right, ctx);
  if (!rightResult.ok) return rightResult;

  // Comparison operators
  const result = compare(leftResult.value, operator, rightResult.value);
  return { ok: true, value: result };
}

function evaluateField(node: ASTNode & { node_type: "field" }, ctx: Context): EvalResult {
  const value = getNestedValue(ctx.data, node.name);
  
  if (value === undefined) {
    return { 
      ok: false, 
      value: null, 
      error: `Missing field: ${node.name}` 
    };
  }
  
  return { ok: true, value };
}

function evaluateLiteral(node: ASTNode & { node_type: "literal" }, _ctx: Context): EvalResult {
  return { ok: true, value: node.value };
}

function evaluateArray(node: ASTNode & { node_type: "array" }, ctx: Context): EvalResult {
  const values: any[] = [];
  
  for (const element of node.elements) {
    const result = evaluate(element, ctx);
    if (!result.ok) return result;
    values.push(result.value);
  }
  
  return { ok: true, value: values };
}

function evaluateFunction(node: ASTNode & { node_type: "function" }, ctx: Context): EvalResult {
  const fn = ctx.functions?.[node.name];
  
  if (!fn) {
    return { 
      ok: false, 
      value: null, 
      error: `Unknown function: ${node.name}` 
    };
  }

  // Evaluate arguments
  const argsResult = evaluateArray(node.args, ctx);
  if (!argsResult.ok) return argsResult;

  try {
    const value = fn(...argsResult.value);
    return { ok: true, value };
  } catch (error) {
    return { 
      ok: false, 
      value: null, 
      error: `Function ${node.name} failed: ${error}` 
    };
  }
}

/**
 * Rule class for convenient usage
 */
export class Rule {
  constructor(private ast: ASTNode) {}

  /**
   * Parse a rule from JSON AST
   */
  static fromJSON(json: string | object): Rule {
    const ast = typeof json === "string" ? JSON.parse(json) : json;
    return new Rule(ast as ASTNode);
  }

  /**
   * Evaluate the rule against a context
   */
  eval(data: Record<string, any>, functions?: Record<string, (...args: any[]) => any>): EvalResult {
    return evaluate(this.ast, { data, functions });
  }

  /**
   * Check if rule passes (convenience method)
   */
  passes(data: Record<string, any>, functions?: Record<string, (...args: any[]) => any>): boolean {
    const result = this.eval(data, functions);
    return result.ok && !isZero(result.value);
  }

  /**
   * Convert AST back to Go-parseable expression string
   */
  toString(): string {
    return stringify(this.ast, true);
  }
}

