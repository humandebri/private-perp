# ADR-0005: TanStack Start and limited UI reuse

- Date: 2026-09-18
- Design status: accepted
- Evidence status: UI infrastructure and demo are subject to local verification; production is unverified.

## Context

Build a high-quality trading UI familiar to HL users while avoiding Next.js.

## Options considered

Start with React, Preact with Vite, Start with a Preact compatibility layer, and forking an entire existing application.

## Decision

Use Start, React, TypeScript, Vite, TanStack Router/Query/Table, Tailwind, and Lightweight Charts. Pin dependencies exactly through pnpm; use type-aware Oxlint, Oxfmt, tsc, Vitest, and Playwright. Do not initially adopt React Compiler. Treat HypeTerminal only as a possible component reference and build the initial UI independently.

## Drawbacks and residual risks

This is not necessarily lighter than Preact. Full HL feature parity or execution speed is not guaranteed. Chart rendering and interaction features are added incrementally. Pin Table to the verified v8 API; test v9 adoption as a separate change.

## Reconsideration criteria

If UI reuse demonstrates a time saving, audit licenses, dependencies, and transport behavior before adopting individual components. Do not automatically switch to Next.js or Preact.

## Source of verification status

Track execution results and unimplemented scope in frontend/README.md and
docs/implementation-status.md. Do not bypass the production gates in Plan.md,
section 16.
