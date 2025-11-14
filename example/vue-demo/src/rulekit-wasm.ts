/**
 * TypeScript wrapper for rulekit WASM module
 * Provides type-safe access to the rulekit Go package compiled to WebAssembly
 */

export interface ParseResult {
  handle?: number
  ast?: any
  ruleString?: string
  error?: string
}

export interface EvalResult {
  ok: boolean
  pass: boolean
  fail: boolean
  value?: any
  error?: string
  evaluatedRule?: string
  evaluatedAST?: any
}

export interface RuleHandle {
  handle: number
  ast: any
  ruleString: string
}

declare global {
  interface Window {
    rulekit?: {
      parse: (rule: string) => ParseResult
      eval: (handle: number, contextJSON: string) => EvalResult
      free: (handle: number) => { ok: boolean; error?: string }
    }
    Go: any
  }
}

class RulekitWASM {
  private go: any = null
  private ready: Promise<void>
  private initialized = false

  constructor() {
    this.ready = this.init()
  }

  private async init() {
    try {
      // Load wasm_exec.js if not already loaded
      if (!window.Go) {
        await this.loadScript('/wasm_exec.js')
      }

      const go = new window.Go()
      this.go = go

      const response = await fetch('/rulekit.wasm')
      if (!response.ok) {
        throw new Error(`Failed to load rulekit.wasm: ${response.statusText}`)
      }

      const buffer = await response.arrayBuffer()
      const result = await WebAssembly.instantiate(buffer, go.importObject)

      // Run the Go program (starts in background)
      go.run(result.instance)

      // Wait for the rulekit global to be set
      let attempts = 0
      while (!window.rulekit && attempts < 50) {
        await new Promise(resolve => setTimeout(resolve, 10))
        attempts++
      }

      if (!window.rulekit) {
        throw new Error('rulekit WASM module failed to initialize')
      }

      this.initialized = true
      console.log('✅ rulekit WASM loaded successfully')
    } catch (error) {
      console.error('❌ Failed to initialize rulekit WASM:', error)
      throw error
    }
  }

  private async loadScript(src: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const script = document.createElement('script')
      script.src = src
      script.onload = () => resolve()
      script.onerror = () => reject(new Error(`Failed to load ${src}`))
      document.head.appendChild(script)
    })
  }

  /**
   * Parse a rule expression
   * @param rule The rule expression string
   * @returns ParseResult with handle, AST, and rule string, or error
   */
  async parse(rule: string): Promise<ParseResult> {
    await this.ready

    if (!window.rulekit) {
      throw new Error('rulekit WASM not initialized')
    }

    const result = window.rulekit.parse(rule)

    // Parse AST from JSON string
    if (result.ast && typeof result.ast === 'string') {
      try {
        result.ast = JSON.parse(result.ast)
      } catch (e) {
        console.error('Failed to parse AST JSON:', e)
      }
    }

    return result
  }

  /**
   * Evaluate a parsed rule with a context
   * @param handle The rule handle from parse()
   * @param context The context object with field values
   * @returns EvalResult with evaluation results
   */
  async eval(handle: number, context: Record<string, any>): Promise<EvalResult> {
    await this.ready

    if (!window.rulekit) {
      throw new Error('rulekit WASM not initialized')
    }

    const contextJSON = JSON.stringify(context)
    const result = window.rulekit.eval(handle, contextJSON)

    // Parse value from JSON string if present
    if (result.value && typeof result.value === 'string') {
      try {
        result.value = JSON.parse(result.value)
      } catch (e) {
        // Keep as string if not valid JSON
      }
    }

    // Parse evaluated AST from JSON string
    if (result.evaluatedAST && typeof result.evaluatedAST === 'string') {
      try {
        result.evaluatedAST = JSON.parse(result.evaluatedAST)
      } catch (e) {
        console.error('Failed to parse evaluated AST JSON:', e)
      }
    }

    return result
  }

  /**
   * Free a rule handle to release memory
   * @param handle The rule handle to free
   */
  async free(handle: number): Promise<void> {
    await this.ready

    if (!window.rulekit) {
      throw new Error('rulekit WASM not initialized')
    }

    const result = window.rulekit.free(handle)
    if (result.error) {
      throw new Error(result.error)
    }
  }

  /**
   * Check if WASM is initialized
   */
  isReady(): boolean {
    return this.initialized
  }

  /**
   * Wait for WASM to be ready
   */
  async waitReady(): Promise<void> {
    await this.ready
  }
}

// Export singleton instance
export const rulekit = new RulekitWASM()

// Export class for advanced usage
export { RulekitWASM }

