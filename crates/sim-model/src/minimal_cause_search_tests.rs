use super::combinations;

#[test]
fn exhaustive_order_finds_a_synthetic_known_minimum_and_lexical_tie() {
    let candidates = ["A", "B", "C", "D"];
    let mut found = None;
    for cardinality in 0..=candidates.len() {
        let mut indices = Vec::new();
        combinations(
            candidates.len(),
            cardinality,
            0,
            &mut Vec::new(),
            &mut indices,
        );
        for subset in indices {
            let ids = subset
                .iter()
                .map(|&index| candidates[index])
                .collect::<Vec<_>>();
            let satisfies = (ids.contains(&"A") && ids.contains(&"C"))
                || (ids.contains(&"B") && ids.contains(&"D"));
            if satisfies {
                found = Some(ids);
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    assert_eq!(found, Some(vec!["A", "C"]));
}
