# Building and Installing the VS Code Extension

## Prerequisites

```bash
# Install VS Code Extension Manager
npm install -g @vscode/vsce
```

## Building

From the `vscode-extension` directory:

```bash
# Install dependencies (if any are added later)
npm install

# Package the extension
vsce package
```

This will create a `.vsix` file (e.g., `rulekit-0.1.1.vsix`).

## Installing Locally

### Method 1: Command Line

```bash
code --install-extension rulekit-0.1.1.vsix
```

### Method 2: VS Code UI

1. Open VS Code
2. Go to Extensions view (Cmd+Shift+X / Ctrl+Shift+X)
3. Click the `...` menu at the top
4. Select "Install from VSIX..."
5. Choose the `rulekit-0.1.0.vsix` file

## Testing

1. Create a new file with `.rk` extension
2. Type some rule expressions
3. Verify syntax highlighting works
4. Try code snippets (type `cidr` and press Tab)

## Publishing to VS Code Marketplace

### One-time Setup

1. Create a [Personal Access Token](https://dev.azure.com/) with Marketplace permissions
2. Create a publisher account at https://marketplace.visualstudio.com/manage

```bash
vsce login <publisher-name>
```

### Publishing

```bash
# Publish a new version
vsce publish

# Or publish with version bump
vsce publish patch  # 0.1.0 -> 0.1.1
vsce publish minor  # 0.1.0 -> 0.2.0
vsce publish major  # 0.1.0 -> 1.0.0
```

## Development

To test changes during development:

1. Open the `vscode-extension` folder in VS Code
2. Press `F5` to launch Extension Development Host
3. Create a `.rk` file in the development window
4. Edit the grammar/config and reload the window to see changes

## Icon

The extension expects an `icon.png` file (128x128px minimum) in the root directory. This is used in the VS Code Marketplace and Extensions view.

You can create one or temporarily use any PNG image renamed to `icon.png`.

