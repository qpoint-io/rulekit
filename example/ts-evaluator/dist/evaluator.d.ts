import type { ASTNode, Context, EvalResult } from "./types.js";
/**
 * Evaluates an AST node against a context
 */
export declare function evaluate(node: ASTNode, ctx: Context): EvalResult;
/**
 * Rule class for convenient usage
 */
export declare class Rule {
    private ast;
    constructor(ast: ASTNode);
    /**
     * Parse a rule from JSON AST
     */
    static fromJSON(json: string | object): Rule;
    /**
     * Evaluate the rule against a context
     */
    eval(data: Record<string, any>, functions?: Record<string, (...args: any[]) => any>): EvalResult;
    /**
     * Check if rule passes (convenience method)
     */
    passes(data: Record<string, any>, functions?: Record<string, (...args: any[]) => any>): boolean;
    /**
     * Convert AST back to Go-parseable expression string
     */
    toString(): string;
}
