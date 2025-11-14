import { StreamLanguage } from "@codemirror/language"

interface State {
  // Can add state if needed for context-aware parsing
}

const ruleLanguage = StreamLanguage.define<State>({
  token(stream, state) {
    // Skip whitespace
    if (stream.eatSpace()) return null
    
    // Line comments: -- comment
    if (stream.match(/^--[^\n]*/)) {
      return "lineComment"
    }
    
    // Block comments: /* comment */
    if (stream.match(/^\/\*[\s\S]*?\*\//)) {
      return "blockComment"
    }
    
    // Handle operators first (before field names)
    // Logical operators
    if (stream.match(/^(!(?!=)|not\b)/i)) return "keyword"
    if (stream.match(/^(&&|and\b)/i)) return "keyword"
    if (stream.match(/^(\|\||or\b)/i)) return "keyword"
    
    // Comparison operators (word forms)
    if (stream.match(/^(eq\b|ne\b|lt\b|le\b|gt\b|ge\b)/i)) return "keyword"
    
    // Special operators
    if (stream.match(/^(contains\b|matches\b|in\b)/i)) return "keyword"
    
    // Comparison operators (symbol forms)
    if (stream.match(/^(==|!=|<=|>=|<|>|=~)/)) return "operator"
    
    // Boolean literals
    if (stream.match(/^(true|false)\b/i)) return "bool"
    
    // Numbers (float before int to catch decimals)
    if (stream.match(/^[+-]?\d*\.\d+/)) return "number"
    if (stream.match(/^[+-]?\d+/)) return "number"
    
    // IPv6 CIDR (complex pattern, needs to come before IPv4)
    if (stream.match(/^[0-9a-fA-F:]+::[0-9a-fA-F:]*\/\d{1,3}/)) return "string.special"
    
    // IPv6 addresses (simplified pattern)
    if (stream.match(/^[0-9a-fA-F]+:[0-9a-fA-F:]+(?::[0-9a-fA-F]+)+/)) return "string.special"
    
    // IPv4 CIDR
    if (stream.match(/^(?:\d{1,3}\.){3}\d{1,3}\/\d{1,2}/)) return "string.special"
    
    // IPv4 addresses
    if (stream.match(/^(?:\d{1,3}\.){3}\d{1,3}/)) return "string.special"
    
    // Hex strings (e.g., 47:45:54 or AB:CD:EF:01:23:45)
    // Must come after IP addresses to avoid conflicts
    if (stream.match(/^[0-9a-fA-F]{2}(?::[0-9a-fA-F]{2})+/)) return "string.special"
    
    // Regex literals
    // Forward slash style: /pattern/
    if (stream.match(/^\/(?:[^\/\\]|\\.)*\//)) return "regexp"
    // Pipe style: |pattern|
    if (stream.match(/^\|(?:[^\|\\]|\\.)*\|/)) return "regexp"
    
    // Strings (double-quoted)
    if (stream.match(/^"(?:[^"\\]|\\[ntr"\\])*"/)) return "string"
    // Strings (single-quoted)
    if (stream.match(/^'(?:[^'\\]|\\[ntr'\\])*'/)) return "string"
    
    // Functions (identifiers followed by parenthesis)
    // Look ahead for opening paren without consuming it
    const beforePos = stream.pos
    if (stream.match(/^[a-zA-Z_][a-zA-Z0-9_]*/)) {
      const nextChar = stream.peek()
      if (nextChar === '(') {
        return "function"
      }
      // Not a function, reset and continue to field matching
      stream.pos = beforePos
    }
    
    // Field names (can contain dots, dashes)
    if (stream.match(/^[a-zA-Z_][a-zA-Z0-9_.\-]*/)) return "propertyName"
    
    // Control characters/punctuation
    if (stream.match(/^[(),\[\]]/)) return "punctuation"
    
    // If we get here, consume one character and move on
    stream.next()
    return null
  },
  
  startState: () => ({}),
})

export { ruleLanguage }

