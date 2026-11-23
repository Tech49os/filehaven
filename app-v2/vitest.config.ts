import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  test: {
    environment: 'happy-dom',
    // 让 ?raw 能读到 tokens.css 源码，供「样式令牌必须在令牌表里有定义」的回归测试使用
    css: true,
    include: ['src/**/*.test.ts'],
  },
})
