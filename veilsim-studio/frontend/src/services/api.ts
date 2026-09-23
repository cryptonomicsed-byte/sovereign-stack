// Typed fetch wrapper for VeilSim Studio backend (port 8788)
// In dev, Vite proxies /api → http://localhost:8788
// In prod (Docker), nginx proxies /api → backend container

const BASE = import.meta.env.PROD ? '/api' : '/api'

// ── Schema types (mirrors backend/main.py) ───────────────────────────────────

export interface SimScenario {
  name: string
  robot_model: string
  trajectory_count: number
  selection_objective: string
  params?: Record<string, unknown>
}

export interface TwinQuality {
  f1_score: number
  area_m2?: number
  frame_count?: number
}

export interface TwinRegion {
  lat_min?: number
  lat_max?: number
  lon_min?: number
  lon_max?: number
  alt_m?: number
}

export interface TwinAsset {
  twin_id: string
  owner_did: string
  creator_did: string
  region?: TwinRegion
  quality: TwinQuality
  robot_model: string
}

export interface RunRequest {
  twin: TwinAsset
  scenario: SimScenario
  veil_id?: string
}

export interface TrajectoryResult {
  trajectory_id: string
  policy_id: string
  success: boolean
  energy_j: number
  risk_score: number
  duration_s: number
  metrics?: Record<string, unknown>
}

export interface SimPolicy {
  id: string
  energy: number
  risk: number
  duration_s: number
  metrics?: Record<string, unknown>
}

export interface OsovmRunResult {
  engine_version: string
  scenario: SimScenario
  trajectories: TrajectoryResult[]
  candidate_policies: SimPolicy[]
  selected_policy_id: string
  run_id: string
  wall_ms: number
  veil_id?: string
  f1_score?: number
  mint_eligible: boolean
}

export interface Veil {
  id: string
  veil_number: number
  name: string
  technical_name: string
  equation: string
  category: string
  description: string
  difficulty: number
  continent: string
  ffi_language: string
  julia_fn: string
  tags: string[]
}

export interface VeilsResponse {
  veils: Veil[]
  total: number
}

export interface HealthResponse {
  status: string
  veils_loaded: number
  version: string
}

export interface DifficultyResponse {
  current_difficulty: number
  genesis: number
}

// ── Fetch helpers ─────────────────────────────────────────────────────────────

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json', ...init?.headers },
    ...init,
  })
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText)
    throw new Error(`API ${res.status}: ${text}`)
  }
  return res.json() as Promise<T>
}

// ── Public API surface ────────────────────────────────────────────────────────

export const api = {
  health(): Promise<HealthResponse> {
    return apiFetch<HealthResponse>('/health')
  },

  difficulty(): Promise<DifficultyResponse> {
    return apiFetch<DifficultyResponse>('/difficulty')
  },

  veils(category?: string, limit = 50): Promise<VeilsResponse> {
    const params = new URLSearchParams({ limit: String(limit) })
    if (category) params.set('category', category)
    return apiFetch<VeilsResponse>(`/veils?${params}`)
  },

  veil(id: string): Promise<Veil> {
    return apiFetch<Veil>(`/veils/${encodeURIComponent(id)}`)
  },

  run(req: RunRequest): Promise<OsovmRunResult> {
    return apiFetch<OsovmRunResult>('/run', {
      method: 'POST',
      body: JSON.stringify(req),
    })
  },
}
