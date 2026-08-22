#[path = "../src/fixtures.rs"]
pub mod fixtures;

const FIXTURE: &str =
    include_str!("../../../../vanilla/fixtures/section/26.2-semantic-fixtures.txt");

#[test]
fn committed_source_backed_fixture_qualifies() {
    let evidence = fixtures::qualify_fixture(FIXTURE).expect("committed fixture must qualify");
    assert_eq!(evidence.cases(), 10);
    assert_eq!(evidence.block_candidate_checks(), 32);
    assert_eq!(evidence.biome_checks(), 2);
}

#[test]
fn fixture_is_deterministic_and_nonempty() {
    let first = fixtures::qualify_fixture(FIXTURE).expect("first fixture qualification");
    let second = fixtures::qualify_fixture(FIXTURE).expect("second fixture qualification");
    assert_eq!(first, second);
    assert_ne!(first.fingerprint(), 0);
}

#[test]
fn fixture_provenance_drift_fails_closed() {
    let changed = FIXTURE.replacen(
        "79e5803347d6fb6f7ffccea4cef783998a1c6469ed869d26fa48ab5f2328cd3b",
        "0000000000000000000000000000000000000000000000000000000000000000",
        1,
    );
    assert!(fixtures::qualify_fixture(&changed).is_err());
}
