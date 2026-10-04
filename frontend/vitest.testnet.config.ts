import { defineConfig } from 'vitest/config'
export default defineConfig({
  test: { include: ['tests/integration/hl-testnet-smoke.test.ts'], testTimeout: 600_000 },
})
