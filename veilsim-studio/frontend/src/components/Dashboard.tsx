import { useEffect, useState } from 'react'
import { api } from '../services/api'
import { useSimContext } from '../context/SimContext'
import VeilSelector from './VeilSelector'
import SimRunner from './SimRunner'
import ProofDisplay from './ProofDisplay'

type Tab = 'runner' | 'history'

function StatusDot({ online }: { online: boolean | null }) {
  if (online === null) return <span className="text-gray-500 text-xs">checking…</span>
  return (
    <span className={`inline-flex items-center gap-1.5 text-xs ${online ? 'text-green-400' : 'text-red-400'}`}>
      <span className={`w-1.5 h-1.5 rounded-full ${online ? 'bg-green-400 animate-pulse' : 'bg-red-400'}`} />
      {online ? 'Backend online' : 'Backend offline'}
    </span>
  )
}

export default function Dashboard() {
  const { backendOnline, setBackendStatus, runs } = useSimContext()
  const [tab, setTab] = useState<Tab>('runner')
  const [difficulty, setDifficulty] = useState<number | null>(null)

  useEffect(() => {
    api.health()
      .then(() => setBackendStatus(true))
      .catch(() => setBackendStatus(false))

    api.difficulty()
      .then(d => setDifficulty(d.current_difficulty))
      .catch(() => {})
  }, [setBackendStatus])

  function handleRunComplete() {
    setTab('history')
  }

  return (
    <div className="min-h-screen bg-[#0a0e1a] text-gray-200 font-mono flex flex-col">
      {/* Top bar */}
      <header className="border-b border-gray-800 px-5 py-3 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <span className="text-indigo-400 font-bold text-base tracking-tight">VeilSim Studio</span>
          <span className="text-gray-600 text-xs hidden sm:inline">Proof-of-Useful-Simulation</span>
        </div>
        <div className="flex items-center gap-4">
          {difficulty !== null && (
            <span className="text-xs text-gray-500">
              Threshold: <span className="text-amber-400 font-semibold">{difficulty.toFixed(3)}</span>
            </span>
          )}
          <StatusDot online={backendOnline} />
        </div>
      </header>

      <div className="flex flex-1 min-h-0">
        {/* Left sidebar — Veil selector */}
        <aside className="w-64 shrink-0 border-r border-gray-800 p-4 flex flex-col overflow-hidden">
          <div className="text-xs text-gray-500 uppercase tracking-wider mb-3 font-semibold">
            Veils
          </div>
          <div className="flex-1 overflow-hidden">
            <VeilSelector />
          </div>
        </aside>

        {/* Main pane */}
        <main className="flex-1 flex flex-col min-w-0 overflow-hidden">
          {/* Tab bar */}
          <div className="border-b border-gray-800 px-5 flex gap-0">
            {(['runner', 'history'] as Tab[]).map(t => (
              <button
                key={t}
                onClick={() => setTab(t)}
                className={`px-4 py-2.5 text-xs font-medium border-b-2 transition-colors capitalize ${
                  tab === t
                    ? 'border-indigo-500 text-indigo-400'
                    : 'border-transparent text-gray-500 hover:text-gray-300'
                }`}
              >
                {t === 'history' ? `History${runs.length ? ` (${runs.length})` : ''}` : 'Runner'}
              </button>
            ))}
          </div>

          {/* Tab content */}
          <div className="flex-1 overflow-y-auto p-5">
            {tab === 'runner' && (
              <div className="max-w-xl">
                <SimRunner onResult={handleRunComplete} />
              </div>
            )}
            {tab === 'history' && (
              <div className="max-w-3xl">
                <ProofDisplay />
              </div>
            )}
          </div>
        </main>
      </div>
    </div>
  )
}
