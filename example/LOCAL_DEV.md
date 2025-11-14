# 🛠️ Local Development (Without Publishing to npm)

There are **three ways** to use the TypeScript evaluator locally without publishing to npm:

## Method 1: Direct Relative Imports (Recommended for Examples)

The Vue demo already uses this approach:

```typescript
// src/App.vue
import { Rule } from '../../ts-evaluator/src/index'
import type { EvalResult } from '../../ts-evaluator/src/types'
```

**Pros:**
- ✅ No setup needed
- ✅ Changes reflect immediately
- ✅ TypeScript works perfectly

**Cons:**
- ❌ Imports look different than production
- ❌ Only works for co-located projects

## Method 2: npm link (Simulates Published Package)

### Setup

```bash
# 1. Build the TypeScript package
cd example/ts-evaluator
npm run build

# 2. Create global symlink
npm link

# 3. Link in your Vue project
cd ../vue-demo
npm link @qpoint/rule-evaluator
```

### Use Normal Imports

```typescript
// Now you can use package imports!
import { Rule } from '@qpoint/rule-evaluator'
import type { EvalResult } from '@qpoint/rule-evaluator'
```

### Cleanup

```bash
# Remove links when done
cd vue-demo
npm unlink @qpoint/rule-evaluator

cd ../ts-evaluator
npm unlink
```

**Pros:**
- ✅ Imports look like production
- ✅ Easy to switch between local and published

**Cons:**
- ❌ Need to rebuild after changes (`npm run build`)
- ❌ Global symlinks can be confusing

## Method 3: Vite Alias (Best for Development)

Already configured in `vue-demo/vite.config.ts`:

```typescript
import path from 'path'

export default defineConfig({
  resolve: {
    alias: {
      '@qpoint/rule-evaluator': path.resolve(__dirname, '../ts-evaluator/src/index.ts')
    }
  }
})
```

Then use package-style imports:

```typescript
import { Rule } from '@qpoint/rule-evaluator'
```

**Pros:**
- ✅ Package-style imports
- ✅ Hot reload works
- ✅ No build step needed

**Cons:**
- ❌ Config needed for each project
- ❌ Different in dev vs production

## Method 4: File Path in package.json

You can also add a local dependency:

```json
{
  "dependencies": {
    "@qpoint/rule-evaluator": "file:../ts-evaluator"
  }
}
```

Then `npm install` will symlink it.

**Pros:**
- ✅ Works like a real dependency
- ✅ Easy to switch to published version

**Cons:**
- ❌ Need to rebuild for changes
- ❌ Can cause issues with different node_modules

## Recommended Approach

**For the Examples:**
- ✅ Use **Method 1** (Direct Imports) - Simple and transparent

**For Your Own Projects:**
- 🚀 Use **Method 2** (npm link) or **Method 3** (Vite Alias)
- When ready, publish and switch to real npm package

## Transition to Published Package

When you're ready to publish:

```bash
# 1. Publish to npm (or private registry)
cd ts-evaluator
npm publish

# 2. In your project, unlink and install
cd ../your-project
npm unlink @qpoint/rule-evaluator  # if using npm link
npm install @qpoint/rule-evaluator

# 3. Remove Vite alias (if using)
# Edit vite.config.ts - remove alias

# 4. Imports stay the same!
import { Rule } from '@qpoint/rule-evaluator'
```

## Current Vue Demo Setup

The `vue-demo` currently uses **Method 1** (direct imports):

```typescript
// src/App.vue
import { Rule } from '../../ts-evaluator/src/index'
```

This is intentional so the example works without any setup! 

To switch to package-style imports, use Method 2 or Method 3 above.

## Troubleshooting

### "Cannot find module '@qpoint/rule-evaluator'"

- If using **Method 1**: Use relative imports `'../../ts-evaluator/src/index'`
- If using **Method 2**: Run `npm link` in both directories
- If using **Method 3**: Check `vite.config.ts` alias path

### TypeScript errors with imports

Add to `tsconfig.json`:

```json
{
  "compilerOptions": {
    "paths": {
      "@qpoint/rule-evaluator": ["../ts-evaluator/src/index.ts"]
    }
  }
}
```

### Changes not reflected

- **Method 1/3**: Should auto-update (HMR)
- **Method 2**: Need to rebuild: `npm run build`

### Vite errors about dependencies

Some frameworks don't like symlinks. Use `vite.config.ts`:

```typescript
export default defineConfig({
  resolve: {
    preserveSymlinks: true
  }
})
```

