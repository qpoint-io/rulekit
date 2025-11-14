# Quick Start Guide

Get syntax highlighting for `.rk` files in VS Code in 3 steps! 🚀

## Install vsce (one-time)

```bash
npm install -g @vscode/vsce
```

## Build & Install

```bash
# From the vscode-extension directory
cd /Users/kamal/code/qpoint/rulekit/vscode-extension

# Package the extension
vsce package

# Install it
code --install-extension rulekit-0.1.1.vsix
```

## Test It

1. Create a new file: `test.rk`
2. Write some rules:

```rulekit
-- This is a comment
http.method == "GET" and http.status >= 200

-- Regex matching
domain matches /^example\.com$/

-- IP CIDR
source.ip == 192.168.0.0/16

-- In operator
status in [200, 201, 204]
```

3. See the beautiful syntax highlighting! 🎨

## Try Snippets

Type these and press **Tab**:
- `cidr` → CIDR match template
- `match` → Regex match template  
- `contains` → Contains check
- `in` → In set check

## That's It!

Now all `.rk` files will have:
- ✅ Syntax highlighting
- ✅ Auto-closing brackets/quotes
- ✅ Comment toggling (Cmd+/)
- ✅ Code snippets
- ✅ Bracket matching

## Uninstall

```bash
code --uninstall-extension qpoint.rulekit
```

## Troubleshooting

**"vsce: command not found"**
```bash
npm install -g @vscode/vsce
```

**Extension not showing up**
1. Restart VS Code completely
2. Check: View → Extensions → Search "rulekit"

**No syntax highlighting**
1. Make sure file has `.rk` extension
2. Check bottom-right language selector shows "Rulekit"
3. If not, click it and select "Rulekit" from the list

