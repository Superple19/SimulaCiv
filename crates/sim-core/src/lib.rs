//! Core primitive types and deterministic coordinate PRNG for SimulaCiv.

pub mod primitives;
pub mod prng;

pub use primitives::{AgentId, DenseSlot, GroupId, Money, ReplicateId, SimulationDay};
pub use prng::{
    F32_NORMALIZER, K_MUL1, K_MUL2, K_PRIME, RngCoordinate, coordinate_prng_f32,
    coordinate_prng_u64, fold_coordinate, mix64, pack_coordinate, u64_to_f32,
};
