import { createFileRoute } from '@tanstack/react-router'
import { TradeApp } from '../ui/local-app'
export const Route = createFileRoute('/trade')({
  ssr: false,
  component: TradeApp,
})
