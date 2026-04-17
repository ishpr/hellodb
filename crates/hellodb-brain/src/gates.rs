//! Firing gates for the brain.
//!
//! The passive-memory pattern depends on NOT firing on every episode. Each
//! pass evaluates a set of gates; all must pass for digestion to happen.
//! The lockfile is a separate concern — it prevents concurrent runs — but
//! these gates prevent over-eager runs.

use crate::config::GatesConfig;
use crate::state::State;

#[derive(Debug, Clone)]
pub enum GateDecision {
    /// All gates passed; proceed with digest.
    Fire,
    /// At least one gate held; skip this pass with a human-readable reason.
    Skip(String),
}

pub fn evaluate(
    state: &State,
    new_episode_count: usize,
    config: &GatesConfig,
    now_ms: u64,
) -> GateDecision {
    // Gate 1: cool-down since last successful run.
    if state.last_run_ms > 0 {
        let elapsed = now_ms.saturating_sub(state.last_run_ms);
        if elapsed < config.min_time_since_last_run_ms {
            return GateDecision::Skip(format!(
                "cool-down: {}ms elapsed, need {}ms (last run was {}ms ago)",
                elapsed, config.min_time_since_last_run_ms, elapsed
            ));
        }
    }
    // Gate 2: enough fresh material to justify a digest call.
    if (new_episode_count as u64) < config.min_episodes_since_last_run {
        return GateDecision::Skip(format!(
            "too few new episodes: {new_episode_count} observed, need {}",
            config.min_episodes_since_last_run
        ));
    }
    GateDecision::Fire
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(min_time: u64, min_ep: u64) -> GatesConfig {
        GatesConfig {
            min_time_since_last_run_ms: min_time,
            min_episodes_since_last_run: min_ep,
        }
    }

    #[test]
    fn first_run_with_enough_episodes_fires() {
        let state = State::default();
        let result = evaluate(&state, 10, &cfg(1000, 5), 5000);
        assert!(matches!(result, GateDecision::Fire));
    }

    #[test]
    fn cooldown_blocks() {
        let state = State {
            last_run_ms: 4000,
            ..Default::default()
        };
        let result = evaluate(&state, 10, &cfg(2000, 5), 5000);
        match result {
            GateDecision::Skip(r) => assert!(r.contains("cool-down")),
            _ => panic!("expected Skip"),
        }
    }

    #[test]
    fn too_few_episodes_blocks() {
        let state = State::default();
        let result = evaluate(&state, 2, &cfg(1000, 5), 5000);
        match result {
            GateDecision::Skip(r) => assert!(r.contains("too few")),
            _ => panic!("expected Skip"),
        }
    }

    #[test]
    fn cooldown_elapsed_and_enough_episodes_fires() {
        let state = State {
            last_run_ms: 1000,
            ..Default::default()
        };
        let result = evaluate(&state, 10, &cfg(2000, 5), 5000);
        assert!(matches!(result, GateDecision::Fire));
    }
}
