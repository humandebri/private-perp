import { createContext, useContext, useMemo, useState, type ReactNode } from 'react'
import { LocalGateway, type LiveData } from '../client/gateway'

type SessionContext = {
  gateway: LocalGateway
  address?: string
  data?: LiveData
  busy: boolean
  error?: string
  login: () => Promise<void>
  logout: () => Promise<void>
  refresh: () => Promise<void>
  run: (action: () => Promise<unknown>) => Promise<void>
}
const Context = createContext<SessionContext | undefined>(undefined)

export function LocalSessionProvider({ children }: { children: ReactNode }) {
  const gateway = useMemo(() => new LocalGateway(), [])
  const [address, setAddress] = useState<string>()
  const [data, setData] = useState<LiveData>()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const execute = async (action: () => Promise<void>) => {
    setBusy(true)
    setError(undefined)
    try {
      await action()
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }
  const refresh = () => execute(async () => setData(await gateway.refresh()))
  const value: SessionContext = {
    gateway,
    address,
    data,
    busy,
    error,
    login: () =>
      execute(async () => {
        const session = await gateway.login()
        setAddress(session.address)
        await gateway.fundingInstructions()
        setData(await gateway.refresh())
      }),
    logout: () =>
      execute(async () => {
        await gateway.logout()
        setAddress(undefined)
        setData(undefined)
      }),
    refresh,
    run: (action) =>
      execute(async () => {
        await action()
        setData(await gateway.refresh())
      }),
  }
  return <Context.Provider value={value}>{children}</Context.Provider>
}

export function useLocalSession(): SessionContext {
  const value = useContext(Context)
  if (!value) throw new Error('LocalSessionProviderがありません')
  return value
}
