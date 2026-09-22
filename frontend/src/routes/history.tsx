import { createFileRoute } from '@tanstack/react-router'
import { HistoryApp } from '../ui/local-app'
export const Route = createFileRoute('/history')({
  ssr: false,
  component: HistoryApp,
})
