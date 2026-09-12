use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One timestamped robot/drone state along a simulated trajectory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryPoint {
    pub t_ms:   u64,
    pub x:      f64,
    pub y:      f64,
    pub z:      f64,
    pub vx:     f64,
    pub vy:     f64,
    pub vz:     f64,
    pub yaw:    f64,
    pub action: Option<String>,
}

/// A complete simulated trajectory — one candidate policy run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    pub traj_id:      String,
    pub twin_id:      String,
    pub agent_id:     String,
    pub points:       Vec<TrajectoryPoint>,
    pub total_cost:   f64,
    pub collision:    bool,
    pub goal_reached: bool,
    pub sim_hash:     String,
    pub created_at:   DateTime<Utc>,
}

/// A candidate generated during the 100k-trajectory sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryCandidate {
    pub traj_id:    String,
    pub score:      f64,
    pub cost:       f64,
    pub feasible:   bool,
    pub sim_hash:   String,
}
