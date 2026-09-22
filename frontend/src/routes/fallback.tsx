import { createFileRoute } from '@tanstack/react-router'
import { FallbackClient } from '../ui/fallback-client'

export const Route = createFileRoute('/fallback')({ component: FallbackClient })
