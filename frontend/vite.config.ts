import { defineConfig, loadEnv } from 'vite'
import { cloudflare } from '@cloudflare/vite-plugin'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import react from '@vitejs/plugin-react'
import tailwind from '@tailwindcss/vite'

export default defineConfig(({ mode }) => ({
  plugins: [
    cloudflare({
      configPath:
        loadEnv(mode, process.cwd(), 'VITE_').VITE_APP_STAGE === 'testnet'
          ? './wrangler.testnet.jsonc'
          : './wrangler.jsonc',
      viteEnvironment: { name: 'ssr' },
    }),
    tanstackStart(),
    react(),
    tailwind(),
  ],
}))
