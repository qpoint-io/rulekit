/**
 * Convert AST back to Go-parseable expression string
 */

import type { ASTNode } from "./types.js";

/**
 * Convert an AST node to a rule expression string
 */
export function stringify(node: ASTNode, isRoot = false): string {
  const result = stringifyNode(node);
  
  // Remove outer parentheses from root node (like Go does)
  if (isRoot && result.startsWith("(") && result.endsWith(")")) {
    return result.slice(1, -1);
  }
  
  return result;
}

function stringifyNode(node: ASTNode): string {
  switch (node.node_type) {
    case "operator":
      return stringifyOperator(node);
    case "field":
      return node.name;
    case "literal":
      return stringifyLiteral(node);
    case "array":
      return stringifyArray(node);
    case "function":
      return stringifyFunction(node);
    default:
      return "<unknown>";
  }
}

function stringifyOperator(node: ASTNode & { node_type: "operator" }): string {
  const { operator, left, right } = node;
  
  // Handle unary NOT operator
  if (operator === "not") {
    if (!right) return "not ()";
    
    // Special formatting for certain combinations
    if (right.node_type === "operator") {
      const rightOp = right as ASTNode & { node_type: "operator" };
      
      // != instead of not ==
      if (rightOp.operator === "eq" || rightOp.operator === "==") {
        return `${stringifyNode(rightOp.left!)} != ${stringifyNode(rightOp.right!)}`;
      }
      
      // not contains
      if (rightOp.operator === "contains") {
        return `${stringifyNode(rightOp.left!)} not contains ${stringifyNode(rightOp.right!)}`;
      }
      
      // not matches
      if (rightOp.operator === "matches" || rightOp.operator === "=~") {
        return `${stringifyNode(rightOp.left!)} not =~ ${stringifyNode(rightOp.right!)}`;
      }
      
      // not in
      if (rightOp.operator === "in") {
        return `${stringifyNode(rightOp.left!)} not in ${stringifyNode(rightOp.right!)}`;
      }
    }
    
    // Field negation: !field (no space)
    if (right.node_type === "field") {
      return `!${stringifyNode(right)}`;
    }
    
    // Default: not (...)
    return `not (${stringifyNode(right)})`;
  }
  
  // Binary operators
  if (!left || !right) {
    return `<invalid ${operator}>`;
  }
  
  const leftStr = stringifyNode(left);
  const rightStr = stringifyNode(right);
  const opStr = operatorToString(operator);
  
  // Only wrap logical operators (and/or) in parentheses
  // Comparison operators don't get parens
  if (operator === "and" || operator === "or") {
    return `(${leftStr} ${opStr} ${rightStr})`;
  }
  
  return `${leftStr} ${opStr} ${rightStr}`;
}

function stringifyLiteral(node: ASTNode & { node_type: "literal" }): string {
  const { type, value } = node;
  
  if (value === null || value === undefined) {
    return "null";
  }
  
  switch (type) {
    case "string":
      // Escape quotes and backslashes
      const escaped = String(value)
        .replace(/\\/g, "\\\\")
        .replace(/"/g, '\\"');
      return `"${escaped}"`;
    
    case "bool":
      return String(value);
    
    case "int":
    case "float":
      return String(value);
    
    case "ip":
      return String(value);
    
    case "cidr":
      return String(value);
    
    case "bytes":
    case "mac":
      // Hex string representation
      if (Array.isArray(value)) {
        return value.map((b: number) => b.toString(16).padStart(2, "0")).join(":");
      }
      return String(value);
    
    case "unknown":
      // For regex patterns
      if (typeof value === "string" && !value.startsWith("/")) {
        return `/${value}/`;
      }
      return String(value);
    
    default:
      return String(value);
  }
}

function stringifyArray(node: ASTNode & { node_type: "array" }): string {
  const elements = node.elements.map(el => stringifyNode(el));
  return `[${elements.join(", ")}]`;
}

function stringifyFunction(node: ASTNode & { node_type: "function" }): string {
  const args = node.args.elements.map(el => stringifyNode(el));
  return `${node.name}(${args.join(", ")})`;
}

/**
 * Convert operator enum to string representation
 */
function operatorToString(op: string): string {
  switch (op) {
    case "eq":
      return "==";
    case "ne":
      return "!=";
    case "gt":
      return ">";
    case "ge":
      return ">=";
    case "lt":
      return "<";
    case "le":
      return "<=";
    case "matches":
      return "=~";
    case "and":
      return "and";
    case "or":
      return "or";
    case "contains":
      return "contains";
    case "in":
      return "in";
    default:
      return op;
  }
}

