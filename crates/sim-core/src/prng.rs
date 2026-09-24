use serde::{Deserialize, Serialize};

/// Golden ratio fractional constant for 64-bit coordinate mixer.
pub const K_PRIME: u64 = 0x9e37_79b9_7f4a_7c15;

/// Stafford Mix13 first multiplier constant.
pub const K_MUL1: u64 = 0xbf58_476d_1ce4_e5b9;

/// Stafford Mix13 second multiplier constant.
pub const K_MUL2: u64 = 0x94d0_49bb_1331_11eb;

/// 2^-24 normalizer to map the upper 24 bits of a u64 to [0.0, 1.0).
pub const F32_NORMALIZER: f32 = 1.0 / 16_777_216.0;

/// Pure 64-bit mixing function (Stafford Mix13).
#[inline]
pub const fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(K_MUL1);
    z = (z ^ (z >> 27)).wrapping_mul(K_MUL2);
    z ^ (z >> 31)
}

/// Generic semantic coordinate for deterministic PRNG evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RngCoordinate {
    pub master_seed: u64,
    pub replicate_id: u32,
    pub day: u32,
    pub phase: u8,
    pub subsystem_id: u16,
    pub agent_id: u32,
    pub draw_index: u32,
}

impl RngCoordinate {
    #[inline]
    pub const fn new(
        master_seed: u64,
        replicate_id: u32,
        day: u32,
        phase: u8,
        subsystem_id: u16,
        agent_id: u32,
        draw_index: u32,
    ) -> Self {
        Self {
            master_seed,
            replicate_id,
            day,
            phase,
            subsystem_id,
            agent_id,
            draw_index,
        }
    }
}

/// Pack generic coordinate fields into four 64-bit words without data loss.
#[inline]
pub const fn pack_coordinate(coord: &RngCoordinate) -> (u64, u64, u64, u64) {
    let w0 = coord.master_seed;
    let w1 = ((coord.replicate_id as u64) << 32) | (coord.day as u64);
    let w2 = ((coord.phase as u64) << 48)
        | ((coord.subsystem_id as u64) << 32)
        | (coord.agent_id as u64);
    let w3 = coord.draw_index as u64;
    (w0, w1, w2, w3)
}

/// Sequentially fold four packed 64-bit words through Mix64 into a single u64 output.
#[inline]
pub const fn fold_coordinate(w0: u64, w1: u64, w2: u64, w3: u64) -> u64 {
    let h0 = w0.wrapping_add(K_PRIME);
    let h1 = mix64(h0 ^ w1);
    let h2 = mix64(h1 ^ w2);
    let h3 = mix64(h2 ^ w3);
    mix64(h3)
}

/// Evaluates the SplitMix64-CoordinateMixer for the given coordinate, returning a u64.
#[inline]
pub const fn coordinate_prng_u64(coord: &RngCoordinate) -> u64 {
    let (w0, w1, w2, w3) = pack_coordinate(coord);
    fold_coordinate(w0, w1, w2, w3)
}

/// Converts a 64-bit PRNG output to a uniform f32 in [0.0, 1.0) using upper 24 bits.
#[inline]
pub fn u64_to_f32(random_value: u64) -> f32 {
    let upper24 = (random_value >> 40) as u32;
    (upper24 as f32) * F32_NORMALIZER
}

/// Evaluates the SplitMix64-CoordinateMixer for the given coordinate, returning an f32 in [0.0, 1.0).
#[inline]
pub fn coordinate_prng_f32(coord: &RngCoordinate) -> f32 {
    u64_to_f32(coordinate_prng_u64(coord))
}
