/**
 * AST Node types matching the Go rulekit output
 */

export type ASTNode = 
  | ASTNodeOperator
  | ASTNodeField
  | ASTNodeLiteral
  | ASTNodeArray
  | ASTNodeFunction;

export interface ASTNodeOperator {
  node_type: "operator";
  operator: string;
  left: ASTNode | null;
  right: ASTNode | null;
}

export interface ASTNodeField {
  node_type: "field";
  name: string;
}

export interface ASTNodeLiteral {
  node_type: "literal";
  type: "int64" | "float64" | "string" | "bool" | "ip" | "cidr" | "bytes" | "mac" | "null" | "unknown";
  value: any;
}

export interface ASTNodeArray {
  node_type: "array";
  elements: ASTNode[];
}

export interface ASTNodeFunction {
  node_type: "function";
  name: string;
  args: ASTNodeArray;
}

/**
 * Context for rule evaluation
 */
export interface Context {
  /** Key-value map of field values */
  data: Record<string, any>;
  /** Custom functions (optional) */
  functions?: Record<string, (...args: any[]) => any>;
}

/**
 * Result of rule evaluation
 */
export interface EvalResult {
  /** The evaluated value */
  value: any;
  /** Whether evaluation succeeded */
  ok: boolean;
  /** Error message if any */
  error?: string;
}

