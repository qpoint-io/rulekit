import { Rule } from "./evaluator.js";
import { cidrContains as cidr_contains } from "./utils.js";

// Example 1: Simple comparison
const ast1 = {
  node_type: "operator",
  operator: "and",
  left: {
    node_type: "operator",
    operator: "eq",
    left: { node_type: "field", name: "port" },
    right: { node_type: "literal", type: "int", value: 8080 }
  },
  right: {
    node_type: "operator",
    operator: "matches",
    left: { node_type: "field", name: "method" },
    right: { node_type: "literal", type: "string", value: "^GET|POST$" }
  }
};

const rule1 = Rule.fromJSON(ast1);
console.log("Example 1: port == 8080 and method matches /^GET|POST$/");
console.log("Pass:", rule1.passes({ port: 8080, method: "GET" })); // true
console.log("Fail:", rule1.passes({ port: 8080, method: "DELETE" })); // false
console.log();

// Example 2: Array contains
const ast2 = {
  node_type: "operator",
  operator: "in",
  left: { node_type: "field", name: "status" },
  right: {
    node_type: "array",
    elements: [
      { node_type: "literal", type: "int", value: 200 },
      { node_type: "literal", type: "int", value: 201 },
      { node_type: "literal", type: "int", value: 204 }
    ]
  }
};

const rule2 = Rule.fromJSON(ast2);
console.log("Example 2: status in [200, 201, 204]");
console.log("Pass:", rule2.passes({ status: 200 })); // true
console.log("Fail:", rule2.passes({ status: 404 })); // false
console.log();

// Example 3: Nested fields
const ast3 = {
  node_type: "operator",
  operator: "and",
  left: {
    node_type: "operator",
    operator: "eq",
    left: { node_type: "field", name: "request.method" },
    right: { node_type: "literal", type: "string", value: "POST" }
  },
  right: {
    node_type: "operator",
    operator: "contains",
    left: { node_type: "field", name: "request.headers.content-type" },
    right: { node_type: "literal", type: "string", value: "json" }
  }
};

const rule3 = Rule.fromJSON(ast3);
console.log("Example 3: request.method == POST and request.headers.content-type contains json");
const data3 = {
  request: {
    method: "POST",
    headers: {
      "content-type": "application/json"
    }
  }
};
console.log("Pass:", rule3.passes(data3)); // true
console.log();

// Example 4: Custom function
const ast4 = {
  node_type: "function",
  name: "cidr_contains",
  args: {
    node_type: "array",
    elements: [
      { node_type: "field", name: "client_ip" },
      { node_type: "literal", type: "string", value: "10.0.0.0/8" }
    ]
  }
};

const rule4 = Rule.fromJSON(ast4);
console.log("Example 4: cidr_contains(client_ip, '10.0.0.0/8')");
console.log("Pass:", rule4.passes(
  { client_ip: "10.1.2.3" },
  { cidr_contains: cidr_contains }
)); // true
console.log("Fail:", rule4.passes(
  { client_ip: "192.168.1.1" },
  { cidr_contains: cidr_contains }
)); // false
console.log();

// Example 5: Complex rule with multiple conditions
const ast5 = {
  node_type: "operator",
  operator: "and",
  left: {
    node_type: "operator",
    operator: "or",
    left: {
      node_type: "operator",
      operator: "contains",
      left: { node_type: "field", name: "tags" },
      right: { node_type: "literal", type: "string", value: "production" }
    },
    right: {
      node_type: "operator",
      operator: "contains",
      left: { node_type: "field", name: "tags" },
      right: { node_type: "literal", type: "string", value: "staging" }
    }
  },
  right: {
    node_type: "operator",
    operator: "gt",
    left: { node_type: "field", name: "priority" },
    right: { node_type: "literal", type: "int", value: 5 }
  }
};

const rule5 = Rule.fromJSON(ast5);
console.log("Example 5: (tags contains 'production' or tags contains 'staging') and priority > 5");
console.log("Pass:", rule5.passes({ tags: ["production", "critical"], priority: 8 })); // true
console.log("Fail:", rule5.passes({ tags: ["development"], priority: 8 })); // false
console.log();

// Example 6: Error handling
const ast6 = {
  node_type: "operator",
  operator: "eq",
  left: { node_type: "field", name: "missing_field" },
  right: { node_type: "literal", type: "int", value: 42 }
};

const rule6 = Rule.fromJSON(ast6);
const result6 = rule6.eval({ other_field: 100 });
console.log("Example 6: Error handling for missing fields");
console.log("Result:", result6);
console.log();

