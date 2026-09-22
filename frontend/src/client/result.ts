export class SubmissionNotSentError extends Error {}

export class CanisterError extends Error {
  constructor(
    readonly code: string,
    readonly detail: unknown,
  ) {
    super(
      `${code}: ${typeof detail === 'object' ? JSON.stringify(detail, (_, value) => (typeof value === 'bigint' ? value.toString() : value)) : typeof detail === 'string' || typeof detail === 'number' || typeof detail === 'boolean' ? String(detail) : ''}`,
    )
  }
}

export function unwrap<T>(result: { Ok: T } | { Err: unknown }): T {
  if ('Ok' in result) return result.Ok
  const entry = Object.entries(result.Err as Record<string, unknown>)[0] ?? ['Unknown', result.Err]
  throw new CanisterError(entry[0], entry[1])
}

export function variantName(value: object): string {
  return Object.keys(value)[0] ?? 'Unknown'
}
