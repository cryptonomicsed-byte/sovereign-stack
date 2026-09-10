"""
VeilSim Studio — FastAPI backend.

This IS the concrete ỌSỌVM execution endpoint.
POST /run → OsovmRunResult (same JSON schema as twin-protocol/src/osovm.rs expects).

sovereign-node wires OsovmEngine::with_endpoint("http://localhost:8788")
and every proof call lands here.
"""
from __future__ import annotations

import asyncio
import hashlib
import json
import os
import time
import uuid
from pathlib import Path
from typing import Any, Dict, List, Optional

from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field

from simulation_engine import SimulationEngine
from veil_processor import VeilProcessor

# ── Bootstrap ─────────────────────────────────────────────────────────────────

DATA_DIR  = Path(__file__).parent.parent / "data"
VEILS_PATH = DATA_DIR / "veils_1_200.json"

with open(VEILS_PATH) as f:
    VEILS_DB: Dict[str, Any] = {v["id"]: v for v in json.load(f)}

veil_processor  = VeilProcessor(veils=VEILS_DB)
sim_engine      = SimulationEngine(veil_processor=veil_processor)

app = FastAPI(
    title="VeilSim Studio — ỌSỌVM Endpoint",
    version="0.1.0",
    description="Proof-of-Useful-Simulation execution layer for the Sovereign Stack",
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

# ── Request / Response types (mirrors osovm.rs) ───────────────────────────────

class SimScenario(BaseModel):
    name:                str
    robot_model:         str
    trajectory_count:    int  = Field(ge=2, le=256)
    selection_objective: str  = "balanced"
    params:              Optional[Dict[str, Any]] = None

class TwinQuality(BaseModel):
    f1_score:    float = 0.8
    area_m2:     float = 100.0
    frame_count: int   = 500

class TwinRegion(BaseModel):
    lat_min:  float = 0.0
    lat_max:  float = 0.001
    lon_min:  float = 0.0
    lon_max:  float = 0.001
    alt_m:    float = 0.0

class TwinAsset(BaseModel):
    twin_id:     str
    owner_did:   str
    creator_did: str
    region:      TwinRegion   = Field(default_factory=TwinRegion)
    quality:     TwinQuality  = Field(default_factory=TwinQuality)
    robot_model: str          = "Go2"

class RunRequest(BaseModel):
    twin:     TwinAsset
    scenario: SimScenario
    veil_id:  Optional[str] = None  # override which Veil drives the challenge

class SimPolicy(BaseModel):
    id:         str
    energy:     float
    risk:       float
    duration_s: float
    metrics:    Optional[Dict[str, Any]] = None

class TrajectoryResult(BaseModel):
    trajectory_id: str
    policy_id:     str
    success:       bool
    energy_j:      float
    risk_score:    float
    duration_s:    float
    metrics:       Optional[Dict[str, Any]] = None

class OsovmRunResult(BaseModel):
    engine_version:     str
    scenario:           SimScenario
    trajectories:       List[TrajectoryResult]
    candidate_policies: List[SimPolicy]
    selected_policy_id: str
    run_id:             str
    wall_ms:            int
    veil_id:            Optional[str] = None
    f1_score:           Optional[float] = None
    mint_eligible:      bool = False

# ── Routes ────────────────────────────────────────────────────────────────────

@app.get("/health")
async def health():
    return {"status": "ok", "veils_loaded": len(VEILS_DB), "version": "0.1.0"}


@app.post("/run", response_model=OsovmRunResult)
async def run_simulation(req: RunRequest) -> OsovmRunResult:
    """
    Main ỌSỌVM execution endpoint.
    Called by sovereign-node's OsovmEngine when endpoint is set.
    """
    t0 = time.monotonic()

    # Pick a Veil — explicit > tile-derived > random from pool
    veil_id = req.veil_id or _veil_for_twin(req.twin)
    veil    = VEILS_DB.get(veil_id)

    result = await sim_engine.run(
        twin=req.twin,
        scenario=req.scenario,
        veil=veil,
    )

    wall_ms = int((time.monotonic() - t0) * 1000)

    # Compute F1 score for this run (gated on quality + novelty)
    f1 = _compute_f1(result["trajectories"], req.twin.quality.f1_score)

    return OsovmRunResult(
        engine_version     = "veilsim/0.1",
        scenario           = req.scenario,
        trajectories       = [TrajectoryResult(**t) for t in result["trajectories"]],
        candidate_policies = [SimPolicy(**p)        for p in result["candidate_policies"]],
        selected_policy_id = result["selected_policy_id"],
        run_id             = result["run_id"],
        wall_ms            = wall_ms,
        veil_id            = veil_id,
        f1_score           = f1,
        mint_eligible      = f1 >= _current_difficulty(),
    )


@app.get("/veils")
async def list_veils(category: Optional[str] = None, limit: int = 50):
    veils = list(VEILS_DB.values())
    if category:
        veils = [v for v in veils if v.get("category", "").lower() == category.lower()]
    return {"veils": veils[:limit], "total": len(veils)}


@app.get("/veils/{veil_id}")
async def get_veil(veil_id: str):
    veil = VEILS_DB.get(veil_id)
    if not veil:
        raise HTTPException(404, f"Veil {veil_id!r} not found")
    return veil


@app.get("/difficulty")
async def get_difficulty():
    return {"current_difficulty": _current_difficulty(), "genesis": 0.777}


# ── Helpers ───────────────────────────────────────────────────────────────────

def _veil_for_twin(twin: TwinAsset) -> str:
    """Derive a Veil ID from the twin's tile coordinates deterministically."""
    h = int(hashlib.sha256(twin.twin_id.encode()).hexdigest(), 16)
    veil_keys = list(VEILS_DB.keys())
    return veil_keys[h % len(veil_keys)]


def _compute_f1(trajectories: List[Dict], twin_quality: float) -> float:
    """F1 score: proportion of successful trajectories weighted by twin quality."""
    if not trajectories:
        return 0.0
    success_rate = sum(1 for t in trajectories if t["success"]) / len(trajectories)
    avg_risk_inv = 1.0 - (sum(t["risk_score"] for t in trajectories) / len(trajectories))
    return round(success_rate * 0.6 * twin_quality + avg_risk_inv * 0.4, 4)


def _current_difficulty() -> float:
    """Genesis: 0.777. Rises toward 0.98 as the network matures."""
    # TODO: read from on-chain difficulty registry
    return 0.777
