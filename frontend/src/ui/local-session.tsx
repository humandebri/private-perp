import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import { LocalGateway } from '../client/gateway'
import { SessionStore, effectiveAge, orderBlockReason } from '../client/session-store'

function useSessionValue() {
  const gateway = useMemo(() => new LocalGateway(), [])
  const store = useMemo(() => new SessionStore(gateway), [gateway])
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot)
  const [now, setNow] = useState(0)
  const [wallNow, setWallNow] = useState(0)
  useEffect(() => {
    let last = -Infinity
    const tick = () => {
      const current = performance.now()
      setNow(current)
      setWallNow(Date.now())
      const interval = document.visibilityState === 'visible' ? 2_000 : 30_000
      if (current - last >= interval) {
        last = current
        void store.refresh()
      }
    }
    tick()
    const timer = window.setInterval(tick, 1_000)
    document.addEventListener('visibilitychange', tick)
    return () => {
      window.clearInterval(timer)
      document.removeEventListener('visibilitychange', tick)
    }
  }, [store, state.generation, state.address])
  return {
    ...state,
    gateway,
    store,
    fresh: effectiveAge(state, now) <= 10_000 && !state.refreshError,
    age: effectiveAge(state, now),
    wallNow,
    orderBlockReason: orderBlockReason(state, now, wallNow),
    login: store.login,
    logout: store.logout,
    refresh: store.refresh,
    run: store.run,
    submit: store.submit,
  }
}
const Context = createContext<ReturnType<typeof useSessionValue> | undefined>(undefined)
export function LocalSessionProvider({ children }: { children: ReactNode }) {
  return <Context.Provider value={useSessionValue()}>{children}</Context.Provider>
}
export function useLocalSession() {
  const value = useContext(Context)
  if (!value) throw new Error('LocalSessionProvider is missing')
  return value
}
