import { createFileRoute } from '@tanstack/react-router'
import { FundsApp } from '../ui/local-app'
export const Route = createFileRoute('/funds')({
  ssr: false,
  component: FundsApp,
})
