use crate::state::WorldState;
use serde::{Deserialize, Serialize};
use sim_core::AgentId;

/// Execution errors for simulation commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandExecutionError {
    MissingSource(AgentId),
    MissingTarget(AgentId),
    InvalidAmount(f32),
    InsufficientSourceFood {
        source: AgentId,
        food: f32,
        amount: f32,
    },
    SelfTransfer(AgentId),
}

impl std::fmt::Display for CommandExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSource(aid) => write!(f, "command source agent missing: {}", aid),
            Self::MissingTarget(aid) => write!(f, "command target agent missing: {}", aid),
            Self::InvalidAmount(amt) => write!(f, "command food amount invalid: {}", amt),
            Self::InsufficientSourceFood {
                source,
                food,
                amount,
            } => write!(
                f,
                "insufficient food for source agent {}: has {}, requires {}",
                source, food, amount
            ),
            Self::SelfTransfer(aid) => write!(f, "cannot execute food transfer to self: {}", aid),
        }
    }
}

impl std::error::Error for CommandExecutionError {}

/// Minimal atomic authoritative command representation for Phase 6B.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    ModifyFood {
        from: AgentId,
        to: AgentId,
        amount: f32,
    },
}

impl Command {
    /// Executes the command atomically on authoritative world state.
    ///
    /// Validates:
    /// - amount is finite and > 0.0
    /// - from != to
    /// - from agent exists and has food >= amount
    /// - to agent exists
    ///
    /// Conserves food strictly: delta_from + delta_to == 0.0.
    pub fn execute(&self, world: &mut WorldState) -> Result<(), CommandExecutionError> {
        match *self {
            Command::ModifyFood { from, to, amount } => {
                if !amount.is_finite() || amount <= 0.0 {
                    return Err(CommandExecutionError::InvalidAmount(amount));
                }
                if from == to {
                    return Err(CommandExecutionError::SelfTransfer(from));
                }

                let from_idx = world
                    .agents
                    .iter()
                    .position(|a| a.agent_id == from)
                    .ok_or(CommandExecutionError::MissingSource(from))?;
                let to_idx = world
                    .agents
                    .iter()
                    .position(|a| a.agent_id == to)
                    .ok_or(CommandExecutionError::MissingTarget(to))?;

                if world.agents[from_idx].food < amount {
                    return Err(CommandExecutionError::InsufficientSourceFood {
                        source: from,
                        food: world.agents[from_idx].food,
                        amount,
                    });
                }

                world.agents[from_idx].food -= amount;
                world.agents[to_idx].food += amount;

                Ok(())
            }
        }
    }
}
