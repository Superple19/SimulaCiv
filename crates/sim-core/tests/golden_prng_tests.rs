#![allow(clippy::excessive_precision)]

use sim_core::{RngCoordinate, coordinate_prng_f32, coordinate_prng_u64, u64_to_f32};

#[test]
fn test_golden_vector_1_zero_coordinate() {
    let coord = RngCoordinate::new(0, 0, 0, 0, 0, 0, 0);
    let val = coordinate_prng_u64(&coord);
    assert_eq!(val, 0x1957a7604e215178);

    let f = coordinate_prng_f32(&coord);
    let expected_f: f32 = 0.09899371862411499;
    assert_eq!(f, expected_f);
}

#[test]
fn test_golden_vector_2_master_seed_1() {
    let coord = RngCoordinate::new(1, 0, 0, 0, 0, 0, 0);
    let val = coordinate_prng_u64(&coord);
    assert_eq!(val, 0x2aa9cfa61473238e);
}

#[test]
fn test_golden_vector_3_pattern_seed() {
    let coord = RngCoordinate::new(0x0123456789abcdef, 0, 0, 0, 0, 0, 0);
    let val = coordinate_prng_u64(&coord);
    assert_eq!(val, 0x7cd5081854be7f81);
}

#[test]
fn test_golden_vector_4_mid_simulation() {
    let coord = RngCoordinate::new(0x0123456789abcdef, 7, 123, 4, 1, 42, 0);
    let val = coordinate_prng_u64(&coord);
    assert_eq!(val, 0x99a401447e7d75dc);

    let f = coordinate_prng_f32(&coord);
    let expected_f: f32 = 0.6001587510108948;
    assert_eq!(f, expected_f);
}

#[test]
fn test_golden_vector_5_theft_target() {
    let coord = RngCoordinate::new(0xdeadbeefcafebabe, 3, 999, 4, 2, 123456, 1);
    let val = coordinate_prng_u64(&coord);
    assert_eq!(val, 0x76397dab79ddd38b);
}

#[test]
fn test_golden_vector_6_boundary_max_values() {
    let coord = RngCoordinate::new(
        0xffffffffffffffff,
        0xffffffff,
        0xffffffff,
        11,
        5,
        0xffffffff,
        0xffffffff,
    );
    let val = coordinate_prng_u64(&coord);
    assert_eq!(val, 0xfa2454cf03d537e6);

    let f = coordinate_prng_f32(&coord);
    let expected_f: f32 = 0.9771168231964111;
    assert_eq!(f, expected_f);
}

#[test]
fn test_golden_float_conversions() {
    let expected_f1: f32 = 0.09899371862411499;
    let expected_f2: f32 = 0.6001587510108948;
    let expected_f3: f32 = 0.9771168231964111;

    assert_eq!(u64_to_f32(0x1957a7604e215178), expected_f1);
    assert_eq!(u64_to_f32(0x99a401447e7d75dc), expected_f2);
    assert_eq!(u64_to_f32(0xfa2454cf03d537e6), expected_f3);
}
