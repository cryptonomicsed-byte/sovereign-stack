import { useState } from 'react'
import { api } from '../services/api'
import type { RunRequest } from '../services/api'
import { useSimContext } from '../context/SimContext'

const ROBOT_MODELS = ['Go2', 'H1', 'G1']
const OBJECTIVES = ['balanced', 'energy', 'speed', 'safety']

function genTwinId(): string {
  return 'twin:' + Math.random().toString(36).slice(2, 10).toUpperCase()
}

export default function SimRunner({ onResult }: { onResult?: () => void }) {
  const { selectedVeil, addRun } = useSimContext()

  const [twinId, setTwinId] = useState(genTwinId())
  const [robotModel, setRobotModel] = useState('Go2')
  const [trajectoryCount, setTrajectoryCount] = useState(8)
  const [objective, setObjective] = useState('balanced')
  const [ownerDid, setOwnerDid] = useState('did:key:zQ3sh')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleRun() {
    setError(null)
    setLoading(true)
    const req: RunRequest = {
      twin: {
        twin_id: twinId,
        owner_did: ownerDid,
        creator_did: ownerDid,
        robot_model: robotModel,
        quality: { f1_score: 0.85 },
      },
      scenario: {
        name: `sim-${Date.now()}`,
        robot_model: robotModel,
        trajectory_count: trajectoryCount,
        selection_objective: objective,
      },
      veil_id: selectedVeil?.id,
    }

    try {
      const result = await api.run(req)
      addRun({ result, veil: selectedVeil ?? undefined, timestamp: Date.now() })
      onResult?.()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Run failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="bg-gray-900 border border-gray-700 rounded-lg p-5 space-y-4">
      <h2 className="text-sm font-semibold text-indigo-400 uppercase tracking-wider">
        Configure Simulation
      </h2>

      {/* Twin ID */}
      <div className="space-y-1">
        <label className="text-xs text-gray-400">Twin ID</label>
        <div className="flex gap-2">
          <input
            value={twinId}
            onChange={e => setTwinId(e.target.value)}
            className="flex-1 px-3 py-1.5 text-xs bg-gray-800 border border-gray-700 rounded
                       text-gray-200 font-mono focus:outline-none focus:border-indigo-500"
          />
          <button
            onClick={() => setTwinId(genTwinId())}
            className="px-2 py-1.5 text-xs bg-gray-700 hover:bg-gray-600 border border-gray-600 rounded text-gray-300 transition-colors"
            title="Generate new ID"
          >
            ↻
          </button>
        </div>
      </div>

      {/* Owner DID */}
      <div className="space-y-1">
        <label className="text-xs text-gray-400">Owner DID</label>
        <input
          value={ownerDid}
          onChange={e => setOwnerDid(e.target.value)}
          className="w-full px-3 py-1.5 text-xs bg-gray-800 border border-gray-700 rounded
                     text-gray-200 font-mono focus:outline-none focus:border-indigo-500"
        />
      </div>

      {/* Robot model + trajectory count */}
      <div className="grid grid-cols-2 gap-3">
        <div className="space-y-1">
          <label className="text-xs text-gray-400">Robot Model</label>
          <select
            value={robotModel}
            onChange={e => setRobotModel(e.target.value)}
            className="w-full px-3 py-1.5 text-xs bg-gray-800 border border-gray-700 rounded
                       text-gray-200 focus:outline-none focus:border-indigo-500"
          >
            {ROBOT_MODELS.map(m => <option key={m}>{m}</option>)}
          </select>
        </div>

        <div className="space-y-1">
          <label className="text-xs text-gray-400">Trajectories ({trajectoryCount})</label>
          <input
            type="range"
            min={2}
            max={32}
            step={2}
            value={trajectoryCount}
            onChange={e => setTrajectoryCount(Number(e.target.value))}
            className="w-full mt-2 accent-indigo-500"
          />
        </div>
      </div>

      {/* Objective */}
      <div className="space-y-1">
        <label className="text-xs text-gray-400">Selection Objective</label>
        <div className="flex gap-2 flex-wrap">
          {OBJECTIVES.map(o => (
            <button
              key={o}
              onClick={() => setObjective(o)}
              className={`px-3 py-1 text-xs rounded border transition-colors ${
                objective === o
                  ? 'bg-indigo-600 border-indigo-500 text-white'
                  : 'bg-gray-800 border-gray-700 text-gray-400 hover:border-gray-500 hover:text-white'
              }`}
            >
              {o}
            </button>
          ))}
        </div>
      </div>

      {/* Selected veil display */}
      {selectedVeil && (
        <div className="px-3 py-2 bg-indigo-900/30 border border-indigo-800 rounded text-xs">
          <span className="text-indigo-400 font-medium">Veil: </span>
          <span className="text-white">{selectedVeil.name}</span>
          <span className="text-gray-500 ml-2">({selectedVeil.technical_name})</span>
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="px-3 py-2 bg-red-900/30 border border-red-800 rounded text-xs text-red-400">
          {error}
        </div>
      )}

      {/* Run button */}
      <button
        onClick={handleRun}
        disabled={loading}
        className={`w-full py-2.5 text-sm font-semibold rounded border transition-all ${
          loading
            ? 'bg-gray-700 border-gray-600 text-gray-500 cursor-not-allowed'
            : 'bg-indigo-600 border-indigo-500 text-white hover:bg-indigo-500 active:bg-indigo-700'
        }`}
      >
        {loading ? (
          <span className="flex items-center justify-center gap-2">
            <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24" fill="none">
              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z" />
            </svg>
            Running simulation…
          </span>
        ) : (
          'Run Simulation →'
        )}
      </button>
    </div>
  )
}
