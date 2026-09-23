import React, { createContext, useContext, useReducer, useCallback } from 'react'
import type { OsovmRunResult, Veil } from '../services/api'

// ── Types ─────────────────────────────────────────────────────────────────────

export interface RunRecord {
  result: OsovmRunResult
  veil?: Veil
  timestamp: number
}

interface SimState {
  runs: RunRecord[]
  selectedVeil: Veil | null
  backendOnline: boolean | null   // null = unchecked
}

type SimAction =
  | { type: 'ADD_RUN'; payload: RunRecord }
  | { type: 'SELECT_VEIL'; payload: Veil | null }
  | { type: 'SET_BACKEND_STATUS'; payload: boolean }
  | { type: 'CLEAR_RUNS' }

interface SimContextValue extends SimState {
  addRun: (record: RunRecord) => void
  selectVeil: (veil: Veil | null) => void
  setBackendStatus: (online: boolean) => void
  clearRuns: () => void
}

// ── Reducer ───────────────────────────────────────────────────────────────────

const MAX_HISTORY = 20

function reducer(state: SimState, action: SimAction): SimState {
  switch (action.type) {
    case 'ADD_RUN':
      return {
        ...state,
        runs: [action.payload, ...state.runs].slice(0, MAX_HISTORY),
      }
    case 'SELECT_VEIL':
      return { ...state, selectedVeil: action.payload }
    case 'SET_BACKEND_STATUS':
      return { ...state, backendOnline: action.payload }
    case 'CLEAR_RUNS':
      return { ...state, runs: [] }
    default:
      return state
  }
}

const initialState: SimState = {
  runs: [],
  selectedVeil: null,
  backendOnline: null,
}

// ── Context ───────────────────────────────────────────────────────────────────

const SimContext = createContext<SimContextValue | null>(null)

export function SimProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState)

  const addRun = useCallback(
    (record: RunRecord) => dispatch({ type: 'ADD_RUN', payload: record }),
    []
  )
  const selectVeil = useCallback(
    (veil: Veil | null) => dispatch({ type: 'SELECT_VEIL', payload: veil }),
    []
  )
  const setBackendStatus = useCallback(
    (online: boolean) => dispatch({ type: 'SET_BACKEND_STATUS', payload: online }),
    []
  )
  const clearRuns = useCallback(() => dispatch({ type: 'CLEAR_RUNS' }), [])

  return (
    <SimContext.Provider value={{ ...state, addRun, selectVeil, setBackendStatus, clearRuns }}>
      {children}
    </SimContext.Provider>
  )
}

export function useSimContext(): SimContextValue {
  const ctx = useContext(SimContext)
  if (!ctx) throw new Error('useSimContext must be used within SimProvider')
  return ctx
}
