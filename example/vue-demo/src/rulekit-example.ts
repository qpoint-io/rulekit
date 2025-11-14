/**
 * Example usage of rulekit WASM
 * 
 * This demonstrates how to parse and evaluate rules using the WASM module
 */

import { rulekit, type ParseResult, type EvalResult } from './rulekit-wasm'

export async function runExamples() {
  console.log('🚀 Running rulekit WASM examples...\n')

  // Wait for WASM to initialize
  await rulekit.waitReady()

  // Example 1: Simple equality check
  console.log('Example 1: Simple equality')
  const rule1 = await rulekit.parse('domain == "example.com"')
  if (rule1.error) {
    console.error('Parse error:', rule1.error)
  } else {
    console.log('Parsed:', rule1.ruleString)
    console.log('AST:', JSON.stringify(rule1.ast, null, 2))

    const result1 = await rulekit.eval(rule1.handle!, { domain: 'example.com' })
    console.log('Result:', result1.pass ? '✅ PASS' : '❌ FAIL')
    console.log('Evaluated:', result1.evaluatedRule)
    console.log()
  }

  // Example 2: Complex expression with AND/OR
  console.log('Example 2: Complex expression')
  const rule2 = await rulekit.parse('(port == 443 || port == 8080) && domain contains "api"')
  if (!rule2.error && rule2.handle) {
    console.log('Parsed:', rule2.ruleString)

    const result2a = await rulekit.eval(rule2.handle, { port: 443, domain: 'api.example.com' })
    console.log('Test 1:', result2a.pass ? '✅ PASS' : '❌ FAIL', '- port:443, domain:api.example.com')

    const result2b = await rulekit.eval(rule2.handle, { port: 80, domain: 'api.example.com' })
    console.log('Test 2:', result2b.pass ? '✅ PASS' : '❌ FAIL', '- port:80, domain:api.example.com')
    console.log()
  }

  // Example 3: Number comparison
  console.log('Example 3: Number comparison')
  const rule3 = await rulekit.parse('status >= 200 && status < 300')
  if (!rule3.error && rule3.handle) {
    const result3 = await rulekit.eval(rule3.handle, { status: 200 })
    console.log('Status 200:', result3.pass ? '✅ PASS' : '❌ FAIL')
    console.log()
  }

  // Example 4: Regex matching
  console.log('Example 4: Regex matching')
  const rule4 = await rulekit.parse('path matches /^\\/api\\/v\\d+/')
  if (!rule4.error && rule4.handle) {
    const result4a = await rulekit.eval(rule4.handle, { path: '/api/v1/users' })
    console.log('Path /api/v1/users:', result4a.pass ? '✅ PASS' : '❌ FAIL')

    const result4b = await rulekit.eval(rule4.handle, { path: '/admin/users' })
    console.log('Path /admin/users:', result4b.pass ? '✅ PASS' : '❌ FAIL')
    console.log()
  }

  // Example 5: Error handling - invalid syntax
  console.log('Example 5: Error handling')
  const rule5 = await rulekit.parse('domain ==')
  if (rule5.error) {
    console.log('❌ Parse error (expected):', rule5.error)
  }
  console.log()

  // Example 6: Array membership
  console.log('Example 6: Array membership')
  const rule6 = await rulekit.parse('method in ["GET", "POST", "PUT"]')
  if (!rule6.error && rule6.handle) {
    const result6a = await rulekit.eval(rule6.handle, { method: 'GET' })
    console.log('Method GET:', result6a.pass ? '✅ PASS' : '❌ FAIL')

    const result6b = await rulekit.eval(rule6.handle, { method: 'DELETE' })
    console.log('Method DELETE:', result6b.pass ? '✅ PASS' : '❌ FAIL')
    console.log()
  }

  console.log('✅ Examples complete!')
}

// Helper function for interactive testing
export async function testRule(ruleString: string, context: Record<string, any>) {
  try {
    const parsed = await rulekit.parse(ruleString)
    
    if (parsed.error) {
      return {
        success: false,
        error: parsed.error
      }
    }

    const result = await rulekit.eval(parsed.handle!, context)

    // Clean up
    await rulekit.free(parsed.handle!)

    return {
      success: true,
      parsed: {
        rule: parsed.ruleString,
        ast: parsed.ast
      },
      result: {
        pass: result.pass,
        fail: result.fail,
        value: result.value,
        evaluatedRule: result.evaluatedRule,
        evaluatedAST: result.evaluatedAST,
        error: result.error
      }
    }
  } catch (error) {
    return {
      success: false,
      error: String(error)
    }
  }
}

