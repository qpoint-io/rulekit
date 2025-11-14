/**
 * Demo: Consume rules exported from Go rulekit
 */

import { readFileSync } from "fs";
import { Rule } from "./evaluator.js";

// Read rules exported from Go
const rulesFile = readFileSync("../go-to-ts-demo/rules.json", "utf-8");
const { rules } = JSON.parse(rulesFile);

console.log("🚀 TypeScript Rule Evaluator Demo\n");
console.log(`Loaded ${rules.length} rules from Go rulekit\n`);
console.log("=".repeat(70));

// Test data
const testCases = [
  {
    name: "HTTP Request 1",
    data: { port: 8080, method: "GET" }
  },
  {
    name: "HTTP Request 2", 
    data: { port: 8080, method: "DELETE" }
  },
  {
    name: "HTTP Response 1",
    data: { status: 200 }
  },
  {
    name: "HTTP Response 2",
    data: { status: 404 }
  },
  {
    name: "Tagged Resource 1",
    data: { tags: ["production", "critical"], priority: 8 }
  },
  {
    name: "Tagged Resource 2",
    data: { tags: ["development"], priority: 8 }
  },
  {
    name: "API Request",
    data: {
      request: {
        method: "POST",
        headers: {
          "content-type": "application/json; charset=utf-8"
        }
      }
    }
  }
];

// Evaluate each rule against test data
for (let i = 0; i < rules.length; i++) {
  const { expression, ast } = rules[i];
  const rule = Rule.fromJSON(ast);
  
  console.log(`\nRule ${i + 1}: ${expression}`);
  console.log("-".repeat(70));
  
  for (const testCase of testCases) {
    const result = rule.eval(testCase.data);
    if (result.ok && result.value !== false) {
      const emoji = result.value ? "✅" : "❌";
      console.log(`  ${emoji} ${testCase.name}: ${result.value}`);
    }
  }
}

console.log("\n" + "=".repeat(70));
console.log("✨ Demo complete!");

