/**
 * Comparison operators
 */
export function compare(left, operator, right) {
    switch (operator) {
        case "eq":
        case "==":
            return equals(left, right);
        case "ne":
        case "!=":
            return !equals(left, right);
        case "gt":
        case ">":
            return greaterThan(left, right);
        case "ge":
        case ">=":
            return greaterThanOrEqual(left, right);
        case "lt":
        case "<":
            return lessThan(left, right);
        case "le":
        case "<=":
            return lessThanOrEqual(left, right);
        case "contains":
            return contains(left, right);
        case "matches":
        case "=~":
            return matches(left, right);
        case "in":
            return contains(right, left); // reverse: x in array === array contains x
        default:
            return false;
    }
}
function equals(left, right) {
    // Handle null/undefined
    if (left === right)
        return true;
    if (left == null || right == null)
        return false;
    // Deep array comparison
    if (Array.isArray(left) && Array.isArray(right)) {
        if (left.length !== right.length)
            return false;
        return left.every((val, i) => equals(val, right[i]));
    }
    // Loose equality for numbers and strings
    return left == right;
}
function greaterThan(left, right) {
    if (typeof left === "number" && typeof right === "number") {
        return left > right;
    }
    if (typeof left === "string" && typeof right === "string") {
        return left > right;
    }
    return false;
}
function greaterThanOrEqual(left, right) {
    return equals(left, right) || greaterThan(left, right);
}
function lessThan(left, right) {
    if (typeof left === "number" && typeof right === "number") {
        return left < right;
    }
    if (typeof left === "string" && typeof right === "string") {
        return left < right;
    }
    return false;
}
function lessThanOrEqual(left, right) {
    return equals(left, right) || lessThan(left, right);
}
function contains(container, item) {
    // String contains substring
    if (typeof container === "string" && typeof item === "string") {
        return container.includes(item);
    }
    // Array contains item
    if (Array.isArray(container)) {
        return container.some(element => equals(element, item));
    }
    // Object has key
    if (typeof container === "object" && container !== null && typeof item === "string") {
        return item in container;
    }
    return false;
}
function matches(value, pattern) {
    // Convert value to string if needed
    const str = typeof value === "string" ? value : String(value);
    // If pattern is already a regex string, use it
    if (typeof pattern === "string") {
        try {
            const regex = new RegExp(pattern);
            return regex.test(str);
        }
        catch {
            return false;
        }
    }
    // Handle array of strings (match any)
    if (Array.isArray(value)) {
        return value.some(v => matches(v, pattern));
    }
    return false;
}
