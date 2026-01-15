/**
 * Convert AST back to Go-parseable expression string
 */
import type { ASTNode } from "./types.js";
/**
 * Convert an AST node to a rule expression string
 */
export declare function stringify(node: ASTNode, isRoot?: boolean): string;
