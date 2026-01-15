/**
 * Utility functions
 */
/**
 * Check if a value is "zero" (falsy in rule context)
 */
export declare function isZero(value: any): boolean;
/**
 * Get nested value from object using dot notation
 * e.g., getNestedValue({a: {b: {c: 1}}}, "a.b.c") => 1
 */
export declare function getNestedValue(obj: Record<string, any>, path: string): any;
/**
 * Parse IP address (basic implementation)
 */
export declare function parseIP(str: string): string | null;
/**
 * Check if IP is in CIDR range (simplified IPv4 implementation)
 */
export declare function cidrContains(ip: string, cidr: string): boolean;
