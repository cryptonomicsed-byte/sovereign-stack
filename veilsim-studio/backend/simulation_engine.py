"""Simulation execution engine — orchestrates Veil-driven physics loops."""
from __future__ import annotations

import asyncio
import hashlib
import math
import random
import time
import uuid
from typing import Any, Dict, List, Optional


class SimulationEngine:
    def __init__(self, veil_processor):
        self.veil_processor = veil_processor

    async def run(
        self,
        twin,
        scenario,
        veil: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        run_id = f"osovm:run:{uuid.uuid4()}"
        n_traj = max(scenario.trajectory_count, 2)
        f1     = twin.quality.f1_score

        # Choose execution strategy from Veil category
        category = (veil or {}).get("category", "control")
        trajectories = await self._run_trajectories(
            run_id, n_traj, f1, category, scenario, veil
        )

        # Collapse to candidate policies (aggregate by policy_id)
        policy_map: Dict[str, Dict] = {}
        for t in trajectories:
            pid = t["policy_id"]
            if pid not in policy_map:
                policy_map[pid] = {"energy": 0.0, "risk": 0.0, "duration_s": 0.0, "n": 0}
            policy_map[pid]["energy"]     += t["energy_j"]
            policy_map[pid]["risk"]       += t["risk_score"]
            policy_map[pid]["duration_s"] += t["duration_s"]
            policy_map[pid]["n"]          += 1

        candidate_policies = [
            {
                "id":         pid,
                "energy":     v["energy"]     / v["n"],
                "risk":       v["risk"]       / v["n"],
                "duration_s": v["duration_s"] / v["n"],
                "metrics":    {"trajectory_count": v["n"]},
            }
            for pid, v in policy_map.items()
        ]

        selected_policy_id = self._select_policy(
            candidate_policies, scenario.selection_objective
        )

        return {
            "run_id":             run_id,
            "trajectories":       trajectories,
            "candidate_policies": candidate_policies,
            "selected_policy_id": selected_policy_id,
        }

    async def _run_trajectories(
        self,
        run_id: str,
        n: int,
        f1: float,
        category: str,
        scenario,
        veil: Optional[Dict],
    ) -> List[Dict]:
        policy_ids = ["policy:conservative", "policy:balanced", "policy:aggressive"]
        results = []

        for i in range(n):
            policy_id = policy_ids[i % 3] if i < 3 else f"policy:variant:{i}"
            noise     = math.sin(i * 0.7) * 0.05
            base_risk = max(0.0, 0.20 - f1 * 0.15 + noise)

            # Veil-specific modifiers
            if veil:
                difficulty = veil.get("difficulty", 0.5)
                base_risk  = min(1.0, base_risk + difficulty * 0.1)

            energy   = 100.0 + math.sin(i * 1.3) * 40.0
            duration = 15.0 + math.cos(i * 0.9) * 6.0
            success  = base_risk < 0.45

            results.append({
                "trajectory_id": f"traj:{run_id}:{i}",
                "policy_id":     policy_id,
                "success":       success,
                "energy_j":      round(energy, 2),
                "risk_score":    round(min(1.0, base_risk), 4),
                "duration_s":    round(duration, 2),
                "metrics": {
                    "f1_grounding": f1,
                    "veil_category": category,
                },
            })

        return results

    def _select_policy(self, policies: List[Dict], objective: str) -> str:
        if not policies:
            return ""
        key_fn = {
            "min_energy":    lambda p: p["energy"],
            "min_risk":      lambda p: p["risk"],
            "max_throughput": lambda p: p["duration_s"],
        }.get(objective, lambda p: p["energy"] * 0.4 + p["risk"] * 60.0)

        return min(policies, key=key_fn)["id"]
