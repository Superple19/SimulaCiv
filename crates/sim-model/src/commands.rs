use crate::state::WorldState;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId, Money};

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
    MissingSettlement(GroupId),
    MissingAgent(AgentId),
    InsufficientBuyerWealth {
        agent_id: AgentId,
        wealth: Money,
        debit: Money,
    },
    InsufficientSellerFood {
        agent_id: AgentId,
        food: f32,
        sold: f32,
    },
    NegativeDebit {
        agent_id: AgentId,
        debit: Money,
    },
    NegativeSellerNet {
        agent_id: AgentId,
        seller_net: Money,
    },
    NegativeTax(Money),
    TreasuryOverflow(GroupId),
    FinancialOverflow,
    FinancialImbalance {
        total_debit: Money,
        total_payout: Money,
        tax_withheld: Money,
    },
    NegativeWelfarePayout {
        agent_id: AgentId,
        payout: Money,
    },
    NegativeTreasuryDebit(Money),
    InsufficientTreasury {
        group_id: GroupId,
        treasury: Money,
        required: Money,
    },
    WealthOverflow(AgentId),
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
            Self::MissingSettlement(gid) => write!(f, "command settlement missing: {}", gid),
            Self::MissingAgent(aid) => write!(f, "command agent missing: {}", aid),
            Self::InsufficientBuyerWealth {
                agent_id,
                wealth,
                debit,
            } => {
                write!(
                    f,
                    "insufficient wealth for buyer {}: has {}, debit {}",
                    agent_id, wealth, debit
                )
            }
            Self::InsufficientSellerFood {
                agent_id,
                food,
                sold,
            } => {
                write!(
                    f,
                    "insufficient food for seller {}: has {}, sold {}",
                    agent_id, food, sold
                )
            }
            Self::NegativeDebit { agent_id, debit } => {
                write!(f, "negative debit for buyer {}: {}", agent_id, debit)
            }
            Self::NegativeSellerNet {
                agent_id,
                seller_net,
            } => {
                write!(f, "negative payout for seller {}: {}", agent_id, seller_net)
            }
            Self::NegativeTax(tax) => write!(f, "negative tax amount: {}", tax),
            Self::TreasuryOverflow(gid) => write!(f, "treasury overflow for settlement {}", gid),
            Self::FinancialOverflow => {
                write!(f, "financial arithmetic overflow during command execution")
            }
            Self::FinancialImbalance {
                total_debit,
                total_payout,
                tax_withheld,
            } => {
                write!(
                    f,
                    "financial imbalance: debits {} != payouts {} + tax {}",
                    total_debit, total_payout, tax_withheld
                )
            }
            Self::NegativeWelfarePayout { agent_id, payout } => {
                write!(
                    f,
                    "negative welfare payout for agent {}: {}",
                    agent_id, payout
                )
            }
            Self::NegativeTreasuryDebit(amt) => {
                write!(f, "negative treasury debit: {}", amt)
            }
            Self::InsufficientTreasury {
                group_id,
                treasury,
                required,
            } => {
                write!(
                    f,
                    "insufficient treasury for group {}: has {}, requires {}",
                    group_id, treasury, required
                )
            }
            Self::WealthOverflow(aid) => {
                write!(f, "wealth overflow for agent {}", aid)
            }
        }
    }
}

impl std::error::Error for CommandExecutionError {}

/// Individual buyer state update in pooled market clearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuyerMarketUpdate {
    pub agent_id: AgentId,
    pub bought: f32,
    pub debit: Money,
}

/// Individual seller state update in pooled market clearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SellerMarketUpdate {
    pub agent_id: AgentId,
    pub sold: f32,
    pub seller_net: Money,
}

/// Individual recipient state update in welfare distribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WelfareRecipientUpdate {
    pub agent_id: AgentId,
    pub payout: Money,
}

/// Minimal atomic authoritative command representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    ModifyFood {
        from: AgentId,
        to: AgentId,
        amount: f32,
    },
    MarketClearance {
        group_id: GroupId,
        buyer_updates: Vec<BuyerMarketUpdate>,
        seller_updates: Vec<SellerMarketUpdate>,
        tax_withheld: Money,
    },
    WelfareDistribution {
        group_id: GroupId,
        recipient_updates: Vec<WelfareRecipientUpdate>,
        treasury_debit: Money,
    },
}

impl Command {
    /// Executes the command atomically on authoritative world state.
    pub fn execute(&self, world: &mut WorldState) -> Result<(), CommandExecutionError> {
        match self {
            Command::ModifyFood { from, to, amount } => {
                let (from, to, amount) = (*from, *to, *amount);
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
            Command::MarketClearance {
                group_id,
                buyer_updates,
                seller_updates,
                tax_withheld,
            } => {
                let settlement_idx = world
                    .settlements
                    .iter()
                    .position(|s| s.group_id == *group_id)
                    .ok_or(CommandExecutionError::MissingSettlement(*group_id))?;

                for b in buyer_updates {
                    if !b.bought.is_finite() || b.bought < 0.0 {
                        return Err(CommandExecutionError::InvalidAmount(b.bought));
                    }
                    if b.debit < 0 {
                        return Err(CommandExecutionError::NegativeDebit {
                            agent_id: b.agent_id,
                            debit: b.debit,
                        });
                    }
                    let agent = world
                        .agents
                        .iter()
                        .find(|a| a.agent_id == b.agent_id)
                        .ok_or(CommandExecutionError::MissingAgent(b.agent_id))?;
                    if agent.wealth < b.debit {
                        return Err(CommandExecutionError::InsufficientBuyerWealth {
                            agent_id: b.agent_id,
                            wealth: agent.wealth,
                            debit: b.debit,
                        });
                    }
                }

                for s in seller_updates {
                    if !s.sold.is_finite() || s.sold < 0.0 {
                        return Err(CommandExecutionError::InvalidAmount(s.sold));
                    }
                    if s.seller_net < 0 {
                        return Err(CommandExecutionError::NegativeSellerNet {
                            agent_id: s.agent_id,
                            seller_net: s.seller_net,
                        });
                    }
                    let agent = world
                        .agents
                        .iter()
                        .find(|a| a.agent_id == s.agent_id)
                        .ok_or(CommandExecutionError::MissingAgent(s.agent_id))?;
                    if agent.food < s.sold {
                        return Err(CommandExecutionError::InsufficientSellerFood {
                            agent_id: s.agent_id,
                            food: agent.food,
                            sold: s.sold,
                        });
                    }
                }

                if *tax_withheld < 0 {
                    return Err(CommandExecutionError::NegativeTax(*tax_withheld));
                }
                world.settlements[settlement_idx]
                    .treasury
                    .checked_add(*tax_withheld)
                    .ok_or(CommandExecutionError::TreasuryOverflow(*group_id))?;

                let mut total_debit: Money = 0;
                for b in buyer_updates {
                    total_debit = total_debit
                        .checked_add(b.debit)
                        .ok_or(CommandExecutionError::FinancialOverflow)?;
                }
                let mut total_payout: Money = 0;
                for s in seller_updates {
                    total_payout = total_payout
                        .checked_add(s.seller_net)
                        .ok_or(CommandExecutionError::FinancialOverflow)?;
                }
                let total_credit = total_payout
                    .checked_add(*tax_withheld)
                    .ok_or(CommandExecutionError::FinancialOverflow)?;

                if total_debit != total_credit {
                    return Err(CommandExecutionError::FinancialImbalance {
                        total_debit,
                        total_payout,
                        tax_withheld: *tax_withheld,
                    });
                }

                for b in buyer_updates {
                    let agent = world
                        .agents
                        .iter_mut()
                        .find(|a| a.agent_id == b.agent_id)
                        .expect("agent existence checked above");
                    agent.food += b.bought;
                    agent.wealth -= b.debit;
                }

                for s in seller_updates {
                    let agent = world
                        .agents
                        .iter_mut()
                        .find(|a| a.agent_id == s.agent_id)
                        .expect("agent existence checked above");
                    agent.food -= s.sold;
                    agent.wealth += s.seller_net;
                }

                world.settlements[settlement_idx].treasury += *tax_withheld;

                Ok(())
            }
            Command::WelfareDistribution {
                group_id,
                recipient_updates,
                treasury_debit,
            } => {
                let settlement_idx = world
                    .settlements
                    .iter()
                    .position(|s| s.group_id == *group_id)
                    .ok_or(CommandExecutionError::MissingSettlement(*group_id))?;

                if *treasury_debit < 0 {
                    return Err(CommandExecutionError::NegativeTreasuryDebit(
                        *treasury_debit,
                    ));
                }

                if world.settlements[settlement_idx].treasury < *treasury_debit {
                    return Err(CommandExecutionError::InsufficientTreasury {
                        group_id: *group_id,
                        treasury: world.settlements[settlement_idx].treasury,
                        required: *treasury_debit,
                    });
                }

                let mut total_payout: Money = 0;
                for r in recipient_updates {
                    if r.payout < 0 {
                        return Err(CommandExecutionError::NegativeWelfarePayout {
                            agent_id: r.agent_id,
                            payout: r.payout,
                        });
                    }
                    total_payout = total_payout
                        .checked_add(r.payout)
                        .ok_or(CommandExecutionError::FinancialOverflow)?;

                    let agent = world
                        .agents
                        .iter()
                        .find(|a| a.agent_id == r.agent_id)
                        .ok_or(CommandExecutionError::MissingAgent(r.agent_id))?;

                    agent
                        .wealth
                        .checked_add(r.payout)
                        .ok_or(CommandExecutionError::WealthOverflow(r.agent_id))?;
                }

                if total_payout != *treasury_debit {
                    return Err(CommandExecutionError::FinancialImbalance {
                        total_debit: *treasury_debit,
                        total_payout,
                        tax_withheld: 0,
                    });
                }

                world.settlements[settlement_idx].treasury -= *treasury_debit;

                for r in recipient_updates {
                    let agent = world
                        .agents
                        .iter_mut()
                        .find(|a| a.agent_id == r.agent_id)
                        .expect("agent existence checked above");
                    agent.wealth += r.payout;
                }

                Ok(())
            }
        }
    }
}
