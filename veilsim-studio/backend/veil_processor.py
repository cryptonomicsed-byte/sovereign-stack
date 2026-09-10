"""Veil processor — maps Veil categories to simulation parameters."""
from __future__ import annotations
from typing import Any, Dict, Optional


CATEGORY_PARAMS = {
    "control": {
        "difficulty_base": 0.4,
        "time_scale":      1.0,
        "energy_factor":   1.0,
    },
    "ml": {
        "difficulty_base": 0.55,
        "time_scale":      1.4,
        "energy_factor":   1.2,
    },
    "physics": {
        "difficulty_base": 0.5,
        "time_scale":      1.2,
        "energy_factor":   0.9,
    },
    "robotics": {
        "difficulty_base": 0.6,
        "time_scale":      0.8,
        "energy_factor":   1.3,
    },
    "perception": {
        "difficulty_base": 0.5,
        "time_scale":      1.1,
        "energy_factor":   1.0,
    },
    "swarm": {
        "difficulty_base": 0.65,
        "time_scale":      1.5,
        "energy_factor":   1.4,
    },
    "embodiment": {
        "difficulty_base": 0.7,
        "time_scale":      1.3,
        "energy_factor":   1.1,
    },
}


class VeilProcessor:
    def __init__(self, veils: Dict[str, Any]):
        self.veils = veils

    def params_for(self, veil: Optional[Dict[str, Any]]) -> Dict[str, Any]:
        if not veil:
            return CATEGORY_PARAMS["control"]
        category = veil.get("category", "control").lower()
        return CATEGORY_PARAMS.get(category, CATEGORY_PARAMS["control"])

    def get_julia_fn(self, veil: Optional[Dict[str, Any]]) -> str:
        """Return the Julia worker function name for this Veil."""
        if not veil:
            return "pid_controller"
        category = veil.get("category", "control").lower()
        return {
            "control":     "pid_controller",
            "ml":          "gradient_descent",
            "physics":     "ode_solver",
            "robotics":    "forward_kinematics",
            "perception":  "optical_flow",
            "swarm":       "flocking_sim",
            "embodiment":  "lqr_controller",
        }.get(category, "pid_controller")
