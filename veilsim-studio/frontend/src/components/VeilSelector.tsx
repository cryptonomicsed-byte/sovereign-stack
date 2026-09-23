import { useEffect, useState } from 'react'
import { api } from '../services/api'
import type { Veil } from '../services/api'
import { useSimContext } from '../context/SimContext'

const CATEGORIES = [
  { key: '', label: 'All' },
  { key: 'control_systems', label: 'Control' },
  { key: 'machine_learning', label: 'ML' },
  { key: 'signal_processing', label: 'Signal' },
  { key: 'robotics', label: 'Robotics' },
  { key: 'computer_vision', label: 'Vision' },
  { key: 'iot_networks', label: 'IoT' },
  { key: 'optimization', label: 'Optim' },
]

function difficultyColor(d: number): string {
  if (d < 0.5) return 'text-green-400'
  if (d < 0.75) return 'text-yellow-400'
  return 'text-red-400'
}

function difficultyBar(d: number): string {
  const pct = Math.round(d * 100)
  if (d < 0.5) return `bg-green-500`
  if (d < 0.75) return `bg-yellow-500`
  return `bg-red-500`
}

export default function VeilSelector() {
  const { selectedVeil, selectVeil } = useSimContext()
  const [veils, setVeils] = useState<Veil[]>([])
  const [category, setCategory] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [search, setSearch] = useState('')

  useEffect(() => {
    setLoading(true)
    setError(null)
    api.veils(category || undefined, 200)
      .then(r => setVeils(r.veils))
      .catch(e => setError(e.message))
      .finally(() => setLoading(false))
  }, [category])

  const filtered = veils.filter(v => {
    if (!search) return true
    const q = search.toLowerCase()
    return (
      v.name.toLowerCase().includes(q) ||
      v.technical_name.toLowerCase().includes(q) ||
      v.tags.some(t => t.includes(q))
    )
  })

  return (
    <div className="flex flex-col h-full">
      {/* Category tabs */}
      <div className="flex flex-wrap gap-1 mb-3">
        {CATEGORIES.map(c => (
          <button
            key={c.key}
            onClick={() => setCategory(c.key)}
            className={`px-2 py-1 text-xs rounded border transition-colors ${
              category === c.key
                ? 'bg-indigo-600 border-indigo-500 text-white'
                : 'bg-gray-800 border-gray-700 text-gray-400 hover:border-indigo-500 hover:text-white'
            }`}
          >
            {c.label}
          </button>
        ))}
      </div>

      {/* Search */}
      <input
        type="text"
        placeholder="Search veils…"
        value={search}
        onChange={e => setSearch(e.target.value)}
        className="w-full mb-3 px-3 py-1.5 text-xs bg-gray-800 border border-gray-700 rounded
                   text-gray-200 placeholder-gray-500 focus:outline-none focus:border-indigo-500"
      />

      {/* Count */}
      <div className="text-xs text-gray-500 mb-2">
        {loading ? 'Loading…' : error ? <span className="text-red-400">{error}</span> : `${filtered.length} veils`}
      </div>

      {/* Grid */}
      <div className="flex-1 overflow-y-auto grid grid-cols-1 gap-2 pr-1">
        {filtered.map(veil => {
          const selected = selectedVeil?.id === veil.id
          const pct = Math.round(veil.difficulty * 100)
          return (
            <button
              key={veil.id}
              onClick={() => selectVeil(selected ? null : veil)}
              className={`text-left p-3 rounded border transition-all ${
                selected
                  ? 'bg-indigo-900/50 border-indigo-500'
                  : 'bg-gray-800/60 border-gray-700 hover:border-gray-500'
              }`}
            >
              <div className="flex items-start justify-between gap-2">
                <div className="flex-1 min-w-0">
                  <div className="text-xs font-medium text-white truncate">{veil.name}</div>
                  <div className="text-xs text-gray-400 truncate">{veil.technical_name}</div>
                </div>
                <span className={`text-xs font-mono shrink-0 ${difficultyColor(veil.difficulty)}`}>
                  {pct}%
                </span>
              </div>

              {/* Difficulty bar */}
              <div className="mt-2 h-0.5 w-full bg-gray-700 rounded overflow-hidden">
                <div
                  className={`h-full ${difficultyBar(veil.difficulty)} rounded`}
                  style={{ width: `${pct}%` }}
                />
              </div>

              {/* Tags */}
              <div className="mt-1.5 flex flex-wrap gap-1">
                {veil.tags.slice(0, 3).map(t => (
                  <span key={t} className="px-1 py-0.5 text-[10px] bg-gray-700 text-gray-400 rounded">
                    {t}
                  </span>
                ))}
              </div>
            </button>
          )
        })}
      </div>
    </div>
  )
}
