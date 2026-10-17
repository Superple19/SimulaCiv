use sim_core::{RngCoordinate, coordinate_prng_f32, coordinate_prng_u64};
use sim_model::M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID;

#[test]
fn relationship_graph_coordinate_has_a_frozen_u64_and_f32_vector() {
    let coordinate = RngCoordinate::new(
        0x0123_4567_89ab_cdef,
        7,
        0,
        0,
        M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID,
        42,
        0,
    );
    let random_u64 = coordinate_prng_u64(&coordinate);
    let random_f32 = coordinate_prng_f32(&coordinate);
    assert_eq!(random_u64, 0x0090_e2a5_75e5_c2b1);
    assert_eq!(random_f32.to_bits(), 0x3b10_e200);
}
