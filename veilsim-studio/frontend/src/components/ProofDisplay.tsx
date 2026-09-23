import type { OsovmRunResult, TrajectoryResult } from '../services/api'
import { useSimContext } from '../context/SimContext'

function F1Badge({ score }: { score: number }) {
  const eligible = score >= 0.777
  return (
    <div className="flex items-center gap-3">
      <div className="text-center">
        <div className={`text-3xl font-bold font-mono ${eligible ? 'text-green-400' : 'text-yellow-400'}`}>
          {score.toFixed(4)}
        </div>
        <div className="text-xs text-gray-500 mt-0.5">F1 Score</div>
      </div>
      <div className={`px-3 py-1.5 rounded-full text-xs font-semibold border ${
        eligible
          ? 'bg-green-900/40 border-green-600 text-green-400'
          : 'bg-yellow-900/40 border-yellow-600 text-yellow-400'
      }`}>
        {eligible ? 'MINT ELIGIBLE' : 'BELOW THRESHOLD'}
      </div>
    </div>
  )
}

function TrajectoryRow({ t, idx }: { t: TrajectoryResult; idx: number }) {
  return (
    <tr className={`border-t border-gray-800 ${t.success ? '' : 'opacity-60'}`}>
      <td className="py-1.5 px-2 text-xs text-gray-500">{idx + 1}</td>
      <td className="py-1.5 px-2 text-xs font-mono text-gray-300 truncate max-w-[120px]">
        {t.trajectory_id.slice(-8)}
      </td>
      <td className="py-1.5 px-2 text-xs font-mono text-gray-400">
        {t.policy_id.slice(-6)}
      </td>
      <td className="py-1.5 px-2 text-xs text-center">
        <span className={`inline-block w-2 h-2 rounded-full ${t.success ? 'bg-green-400' : 'bg-red-400'}`} />
      </td>
      <td className="py-1.5 px-2 text-xs font-mono text-right text-amber-400">
        {t.energy_j.toFixed(1)}
      </td>
      <td className="py-1.5 px-2 text-xs font-mono text-right text-red-400">
        {t.risk_score.toFixed(3)}
      </td>
      <td className="py-1.5 px-2 text-xs font-mono text-right text-blue-400">
        {t.duration_s.toFixed(2)}s
      </td>
    </tr>
  )
}

function RunCard({ result, veilName, ts }: {
  result: OsovmRunResult
  veilName?: string
  ts: number
}) {
  const f1 = result.f1_score ?? 0
  const successCount = result.trajectories.filter(t => t.success).length

  return (
    <div className="bg-gray-900 border border-gray-700 rounded-lg p-5 space-y-4">
      {/* Header row */}
      <div className="flex items-start justify-between gap-4 flex-wrap">
        <F1Badge score={f1} />
        <div className="text-right space-y-0.5">
          <div className="text-xs text-gray-500 font-mono">{result.run_id}</div>
          <div className="text-xs text-gray-500">{new Date(ts).toLocaleTimeString()}</div>
          <div className="text-xs text-gray-500">{result.wall_ms}ms</div>
        </div>
      </div>

      {/* Meta */}
      <div className="grid grid-cols-2 gap-2 text-xs">
        <div className="bg-gray-800 rounded px-3 py-2">
          <div className="text-gray-500">Robot</div>
          <div className="text-white font-semibold">{result.scenario.robot_model}</div>
        </div>
        <div className="bg-gray-800 rounded px-3 py-2">
          <div className="text-gray-500">Policy</div>
          <div className="text-indigo-400 font-mono truncate">{result.selected_policy_id}</div>
        </div>
        <div className="bg-gray-800 rounded px-3 py-2">
          <div className="text-gray-500">Success Rate</div>
          <div className="text-green-400 font-semibold">
            {successCount}/{result.trajectories.length}
          </div>
        </div>
        <div className="bg-gray-800 rounded px-3 py-2">
          <div className="text-gray-500">Veil</div>
          <div className="text-purple-400 truncate">{veilName ?? result.veil_id ?? '—'}</div>
        </div>
      </div>

      {/* Trajectory table */}
      {result.trajectories.length > 0 && (
        <div className="overflow-x-auto rounded border border-gray-800">
          <table className="w-full text-xs min-w-[480px]">
            <thead>
              <tr className="bg-gray-800 text-gray-500 text-left">
                <th className="py-1.5 px-2 w-8">#</th>
                <th className="py-1.5 px-2">ID</th>
                <th className="py-1.5 px-2">Policy</th>
                <th className="py-1.5 px-2 text-center">OK</th>
                <th className="py-1.5 px-2 text-right">Energy J</th>
                <th className="py-1.5 px-2 text-right">Risk</th>
                <th className="py-1.5 px-2 text-right">Duration</th>
              </tr>
            </thead>
            <tbody>
              {result.trajectories.map((t, i) => (
                <TrajectoryRow key={t.trajectory_id} t={t} idx={i} />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Engine tag */}
      <div className="text-right">
        <span className="text-[10px] text-gray-600 font-mono">{result.engine_version}</span>
      </div>
    </div>
  )
}

export default function ProofDisplay() {
  const { runs, clearRuns } = useSimContext()

  if (runs.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-48 text-center">
        <div className="text-4xl mb-3">⬡</div>
        <div className="text-sm text-gray-500">No simulations run yet.</div>
        <div className="text-xs text-gray-600 mt-1">Configure and run a simulation to see Proof-of-Simulation results.</div>
      </div>
    )
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold text-indigo-400 uppercase tracking-wider">
          Proof-of-Simulation Results ({runs.length})
        </h2>
        <button
          onClick={clearRuns}
          className="text-xs text-gray-500 hover:text-red-400 transition-colors"
        >
          Clear history
        </button>
      </div>
      {runs.map(r => (
        <RunCard
          key={r.result.run_id}
          result={r.result}
          veilName={r.veil?.name}
          ts={r.timestamp}
        />
      ))}
    </div>
  )
}
