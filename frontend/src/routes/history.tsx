import { createFileRoute } from '@tanstack/react-router'
import { DemoApp } from '../ui/demo-app'
export const Route = createFileRoute('/history')({
  ssr: false,
  component: () => <DemoApp screen="history" />,
})
