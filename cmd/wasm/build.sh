#!/bin/bash
set -e

echo "🔨 Building rulekit WASM..."

# Get the directory of this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RULEKIT_ROOT="$SCRIPT_DIR/../.."
OUTPUT_DIR="$RULEKIT_ROOT/example/vue-demo/public"

# Create output directory if it doesn't exist
mkdir -p "$OUTPUT_DIR"

# Build the WASM binary
echo "   Compiling Go to WASM..."
cd "$SCRIPT_DIR"
GOOS=js GOARCH=wasm go build -o "$OUTPUT_DIR/rulekit.wasm" .

# Copy the Go WASM runtime
echo "   Copying wasm_exec.js..."
# Try new location first (Go 1.21+), fall back to old location
if [ -f "$(go env GOROOT)/lib/wasm/wasm_exec.js" ]; then
    cp "$(go env GOROOT)/lib/wasm/wasm_exec.js" "$OUTPUT_DIR/"
elif [ -f "$(go env GOROOT)/misc/wasm/wasm_exec.js" ]; then
    cp "$(go env GOROOT)/misc/wasm/wasm_exec.js" "$OUTPUT_DIR/"
else
    echo "   ❌ Error: wasm_exec.js not found in Go installation"
    exit 1
fi

# Get the size of the WASM file
WASM_SIZE=$(du -h "$OUTPUT_DIR/rulekit.wasm" | cut -f1)

echo "✅ Build complete!"
echo "   WASM file: $OUTPUT_DIR/rulekit.wasm ($WASM_SIZE)"
echo "   Runtime:   $OUTPUT_DIR/wasm_exec.js"
echo ""
echo "💡 To test: cd $RULEKIT_ROOT/example/vue-demo && npm run dev"

