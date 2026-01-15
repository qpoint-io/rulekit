/**
 * Utility functions
 */
/**
 * Check if a value is "zero" (falsy in rule context)
 */
export function isZero(value) {
    if (value === null || value === undefined)
        return true;
    if (typeof value === "boolean")
        return !value;
    if (typeof value === "number")
        return value === 0;
    if (typeof value === "string")
        return value === "";
    if (Array.isArray(value))
        return value.length === 0;
    if (typeof value === "object")
        return Object.keys(value).length === 0;
    return false;
}
/**
 * Get nested value from object using dot notation
 * e.g., getNestedValue({a: {b: {c: 1}}}, "a.b.c") => 1
 */
export function getNestedValue(obj, path) {
    // First try direct key access (most common case)
    if (path in obj) {
        return obj[path];
    }
    // Try nested path access
    const parts = path.split(".");
    let current = obj;
    for (const part of parts) {
        if (current == null || typeof current !== "object") {
            return undefined;
        }
        current = current[part];
    }
    return current;
}
/**
 * Parse IP address (basic implementation)
 */
export function parseIP(str) {
    // IPv4 pattern
    const ipv4Pattern = /^(\d{1,3}\.){3}\d{1,3}$/;
    // IPv6 pattern (simplified)
    const ipv6Pattern = /^([0-9a-fA-F]{0,4}:){2,7}[0-9a-fA-F]{0,4}$/;
    if (ipv4Pattern.test(str) || ipv6Pattern.test(str)) {
        return str;
    }
    return null;
}
/**
 * Check if IP is in CIDR range (simplified IPv4 implementation)
 */
export function cidrContains(ip, cidr) {
    try {
        // Parse CIDR
        const [network, bits] = cidr.split("/");
        const prefixLength = parseInt(bits, 10);
        if (isNaN(prefixLength) || prefixLength < 0 || prefixLength > 32) {
            return false;
        }
        // Convert IPv4 to 32-bit integer
        const ipToInt = (ipStr) => {
            const parts = ipStr.split(".").map(Number);
            if (parts.length !== 4 || parts.some(p => isNaN(p) || p < 0 || p > 255)) {
                return -1;
            }
            return (parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3];
        };
        const ipInt = ipToInt(ip);
        const networkInt = ipToInt(network);
        if (ipInt === -1 || networkInt === -1) {
            return false;
        }
        // Create mask
        const mask = prefixLength === 0 ? 0 : (~0 << (32 - prefixLength));
        // Check if IP is in network
        return (ipInt & mask) === (networkInt & mask);
    }
    catch {
        return false;
    }
}
