import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import path from 'path'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@qpoint/rule-evaluator': path.resolve(__dirname, '../ts-evaluator/src/index.ts')
    }
  },
  server: {
    allowedHosts: ['localhost', '127.0.0.1', '0.0.0.0', '.trycloudflare.com'],
    proxy: {
      '/api': {
        target: 'http://0.0.0.0:10002/devtools',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api/, '/api')
      }
    }
  }
})

