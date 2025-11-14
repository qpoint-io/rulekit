# Rulekit VS Code Extension - Overview

This directory contains a complete VS Code extension for Rulekit syntax highlighting.

## 📁 File Structure

```
vscode-extension/
├── package.json                    # Extension manifest
├── language-configuration.json     # Editor behaviors (brackets, comments)
├── syntaxes/
│   └── rulekit.tmLanguage.json    # TextMate grammar (syntax highlighting)
├── snippets/
│   └── rulekit.json               # Code snippets
├── examples/
│   └── demo.rk                    # Example file showing all features
├── README.md                       # Extension documentation
├── CHANGELOG.md                    # Version history
├── BUILD.md                        # Build and installation guide
├── .vscodeignore                  # Files to exclude from package
├── .gitignore                     # Git ignore patterns
└── icon.png                       # Extension icon (needs to be added)
```

## 🎨 What's Included

### Syntax Highlighting
- Keywords: `and`, `or`, `not`, `in`, `contains`, `matches`
- Operators: `==`, `!=`, `<`, `>`, `<=`, `>=`, `=~`
- Literals: strings, numbers, booleans, IPs, CIDR, hex strings, regex
- Comments: line (`--`) and block (`/* */`)
- Field names and function calls
- All case-insensitive variants

### Editor Features
- Auto-closing pairs for `()`, `[]`, `""`, `''`, `//`, `||`
- Bracket matching
- Comment toggling (Cmd+/ or Ctrl+/)
- Code snippets with tab completion
- Word pattern recognition for field names

### Code Snippets
Type and press Tab:
- `cidr` → IP CIDR match
- `match` → Regex match
- `contains` → Contains check
- `in` → In set check
- `and`/`or`/`not` → Logical operators
- And more...

## 🚀 Quick Start

### 1. Build the Extension

```bash
cd vscode-extension
npm install -g @vscode/vsce
vsce package
```

This creates `rulekit-0.1.0.vsix`

### 2. Install

```bash
code --install-extension rulekit-0.1.0.vsix
```

### 3. Test

Create a file named `test.rk` and start writing rules!

## 🎯 Design Decisions

### Why `.rk` extension?
- Short and memorable
- Not commonly used by other languages
- Easy to type

### TextMate Grammar vs Language Server
This extension uses TextMate grammar for simplicity:
- ✅ Works immediately, no setup required
- ✅ Fast and lightweight
- ✅ Easy to maintain
- ❌ No semantic analysis (can add Language Server later)

### Color Scopes
The grammar uses standard TextMate scopes that work with all themes:
- `keyword.control.logical` - and/or/not
- `keyword.operator.comparison` - eq/ne/lt/gt
- `constant.numeric` - numbers
- `string.quoted` - strings
- `string.regexp` - regex patterns
- `constant.other.ip` - IP addresses
- `variable.other.property` - field names
- `entity.name.function` - function names
- `comment.line` / `comment.block` - comments

## 🔧 Customization

### Adding New Keywords
Edit `syntaxes/rulekit.tmLanguage.json`, add to the `keywords` section:

```json
{
  "name": "keyword.new.rulekit",
  "match": "\\b(?i:newkeyword)\\b"
}
```

### Adding New Snippets
Edit `snippets/rulekit.json`:

```json
{
  "Snippet Name": {
    "prefix": "trigger",
    "body": ["code here"],
    "description": "What it does"
  }
}
```

### Changing Colors
Users can customize colors in their VS Code `settings.json`:

```json
{
  "editor.tokenColorCustomizations": {
    "textMateRules": [
      {
        "scope": "keyword.control.logical.rulekit",
        "settings": {
          "foreground": "#FF0000"
        }
      }
    ]
  }
}
```

## 📦 Publishing to Marketplace

### Prerequisites
1. Microsoft/Azure DevOps account
2. Personal Access Token with Marketplace permissions
3. Publisher account at marketplace.visualstudio.com

### Steps

```bash
# Login
vsce login qpoint

# Publish
vsce publish
```

See [BUILD.md](BUILD.md) for detailed instructions.

## 🔮 Future Enhancements

### Short Term
- [ ] Add icon.png (128x128)
- [ ] Test with various VS Code themes
- [ ] Add more code snippets based on usage

### Medium Term
- [ ] Syntax validation (red squiggles for errors)
- [ ] Hover tooltips for operators/functions
- [ ] Bracket colorization support
- [ ] Semantic highlighting

### Long Term
- [ ] Language Server Protocol implementation
- [ ] Auto-completion based on schema
- [ ] Go to definition for fields
- [ ] Real-time evaluation/testing
- [ ] Integration with Go parser via HTTP

## 🐛 Known Limitations

1. **No syntax validation** - Invalid rules won't show errors (yet)
2. **No autocomplete** - Only snippets, not context-aware suggestions
3. **IPv6 pattern** - Simplified regex, may match some invalid addresses
4. **Regex delimiter** - Pipe `|` conflicts with OR operator (handled but imperfect)

## 📚 Resources

- [VS Code Extension API](https://code.visualstudio.com/api)
- [TextMate Grammar Guide](https://macromates.com/manual/en/language_grammars)
- [Extension Publishing](https://code.visualstudio.com/api/working-with-extensions/publishing-extension)
- [vsce Documentation](https://github.com/microsoft/vscode-vsce)

## 🤝 Contributing

To improve the extension:

1. Edit the appropriate files
2. Test with `F5` (Extension Development Host)
3. Update version in `package.json`
4. Update `CHANGELOG.md`
5. Rebuild with `vsce package`

## ✨ Credits

Based on the Rulekit language specification from lexer.rl.

