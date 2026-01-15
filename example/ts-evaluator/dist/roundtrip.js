/**
 * Round-trip test: Go → JSON → TypeScript → String → Go
 * Demonstrates that AST can be converted back to parseable text
 */
import { readFileSync, writeFileSync } from "fs";
import { Rule } from "./evaluator.js";
console.log("🔄 Round-trip Test: Go → JSON → TS → String → Go\n");
console.log("=".repeat(70));
// Read rules exported from Go
const rulesFile = readFileSync("../go-to-ts-demo/rules.json", "utf-8");
const { rules } = JSON.parse(rulesFile);
const results = [];
for (let i = 0; i < rules.length; i++) {
    const { expression, ast } = rules[i];
    const rule = Rule.fromJSON(ast);
    const regenerated = rule.toString();
    const match = expression === regenerated;
    console.log(`\n${match ? "✅" : "❌"} Rule ${i + 1}`);
    console.log(`  Original:    ${expression}`);
    console.log(`  Regenerated: ${regenerated}`);
    console.log(`  Match: ${match}`);
    results.push({
        original: expression,
        regenerated: regenerated,
        match: match
    });
}
// Save results for Go to verify
const output = {
    total: results.length,
    passed: results.filter(r => r.match).length,
    results: results
};
writeFileSync("../go-to-ts-demo/roundtrip-results.json", JSON.stringify(output, null, 2));
console.log("\n" + "=".repeat(70));
console.log(`\n📊 Results: ${output.passed}/${output.total} rules match exactly`);
console.log(`\n💾 Results saved to roundtrip-results.json`);
if (output.passed === output.total) {
    console.log("🎉 Perfect round-trip! All rules regenerated exactly.");
}
else {
    console.log("⚠️  Some rules differ (may be due to formatting differences)");
}
