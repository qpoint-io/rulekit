import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import path from 'path'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@qpoint/rule-evaluator': path.resolve(__dirname, '../ts-evaluator/src/index.ts')
    }
  }
})

