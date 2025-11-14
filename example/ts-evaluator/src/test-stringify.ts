/**
 * Test stringify functionality
 */

import { Rule } from "./evaluator.js";

console.log("🧪 Testing AST → String Conversion\n");
console.log("=".repeat(70));

const testCases = [
  {
    name: "Simple equality",
    ast: {
      node_type: "operator",
      operator: "eq",
      left: { node_type: "field", name: "port" },
      right: { node_type: "literal", type: "int", value: 8080 }
    },
    expected: "port == 8080"
  },
  {
    name: "AND with match",
    ast: {
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
        right: { node_type: "literal", type: "unknown", value: "^GET|POST$" }
      }
    },
    expected: "port == 8080 and method =~ /^GET|POST$/"
  },
  {
    name: "IN operator with array",
    ast: {
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
    },
    expected: "status in [200, 201, 204]"
  },
  {
    name: "NOT operator (inequality)",
    ast: {
      node_type: "operator",
      operator: "not",
      left: null,
      right: {
        node_type: "operator",
        operator: "eq",
        left: { node_type: "field", name: "status" },
        right: { node_type: "literal", type: "int", value: 200 }
      }
    },
    expected: "status != 200"
  },
  {
    name: "NOT contains",
    ast: {
      node_type: "operator",
      operator: "not",
      left: null,
      right: {
        node_type: "operator",
        operator: "contains",
        left: { node_type: "field", name: "tags" },
        right: { node_type: "literal", type: "string", value: "test" }
      }
    },
    expected: 'tags not contains "test"'
  },
  {
    name: "Field negation",
    ast: {
      node_type: "operator",
      operator: "not",
      left: null,
      right: { node_type: "field", name: "blocked" }
    },
    expected: "!blocked"
  },
  {
    name: "Complex OR with nested fields",
    ast: {
      node_type: "operator",
      operator: "or",
      left: {
        node_type: "operator",
        operator: "contains",
        left: { node_type: "field", name: "host" },
        right: { node_type: "literal", type: "string", value: "example.com" }
      },
      right: {
        node_type: "operator",
        operator: "contains",
        left: { node_type: "field", name: "host" },
        right: { node_type: "literal", type: "string", value: "qpoint.io" }
      }
    },
    expected: 'host contains "example.com" or host contains "qpoint.io"'
  },
  {
    name: "Nested field access",
    ast: {
      node_type: "operator",
      operator: "eq",
      left: { node_type: "field", name: "request.method" },
      right: { node_type: "literal", type: "string", value: "POST" }
    },
    expected: 'request.method == "POST"'
  },
  {
    name: "String with quotes",
    ast: {
      node_type: "operator",
      operator: "eq",
      left: { node_type: "field", name: "message" },
      right: { node_type: "literal", type: "string", value: 'He said "hello"' }
    },
    expected: 'message == "He said \\"hello\\""'
  },
  {
    name: "Function call",
    ast: {
      node_type: "function",
      name: "cidr_contains",
      args: {
        node_type: "array",
        elements: [
          { node_type: "field", name: "client_ip" },
          { node_type: "literal", type: "string", value: "10.0.0.0/8" }
        ]
      }
    },
    expected: 'cidr_contains(client_ip, "10.0.0.0/8")'
  },
  {
    name: "Comparison operators",
    ast: {
      node_type: "operator",
      operator: "and",
      left: {
        node_type: "operator",
        operator: "gt",
        left: { node_type: "field", name: "priority" },
        right: { node_type: "literal", type: "int", value: 5 }
      },
      right: {
        node_type: "operator",
        operator: "le",
        left: { node_type: "field", name: "priority" },
        right: { node_type: "literal", type: "int", value: 10 }
      }
    },
    expected: "priority > 5 and priority <= 10"
  }
];

let passed = 0;
let failed = 0;

for (const testCase of testCases) {
  const rule = Rule.fromJSON(testCase.ast);
  const result = rule.toString();
  const success = result === testCase.expected;
  
  console.log(`\n${success ? "✅" : "❌"} ${testCase.name}`);
  console.log(`   Expected: ${testCase.expected}`);
  console.log(`   Got:      ${result}`);
  
  if (success) {
    passed++;
  } else {
    failed++;
  }
}

console.log("\n" + "=".repeat(70));
console.log(`\n📊 Results: ${passed} passed, ${failed} failed out of ${testCases.length} tests`);

if (failed === 0) {
  console.log("🎉 All tests passed!");
} else {
  console.log("❌ Some tests failed");
  process.exit(1);
}

