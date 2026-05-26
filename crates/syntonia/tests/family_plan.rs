//! Integration test: load and validate the family plan TOML fixture.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration test — panics are the correct failure mode"
)]

use syntonia::{FrequencyPlan, ValidationIssue, baofeng_uv5r_constraints, validate_plan};

#[test]
fn load_family_plan_fixture() {
    let toml_str = include_str!("fixtures/family-plan.toml");
    let plan = FrequencyPlan::from_toml(toml_str).expect("fixture should parse");
    assert_eq!(plan.name, "Family Plan");
    assert_eq!(plan.channel_count(), 10);
}

#[test]
fn family_plan_channels_have_expected_names() {
    let toml_str = include_str!("fixtures/family-plan.toml");
    let plan = FrequencyPlan::from_toml(toml_str).unwrap();

    let names: Vec<&str> = plan.channels.iter().map(|ch| ch.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "CALL", "RPT-1", "RPT-2", "SIMP-1", "SIMP-2", "UHF-1", "UHF-2", "WX", "RPT-3", "EMRG"
        ]
    );
}

#[test]
fn family_plan_validates_with_no_errors() {
    let toml_str = include_str!("fixtures/family-plan.toml");
    let plan = FrequencyPlan::from_toml(toml_str).unwrap();
    let constraints = baofeng_uv5r_constraints();
    let issues = validate_plan(&plan, &constraints);

    let errors: Vec<_> = issues
        .iter()
        .filter(|i| matches!(i, ValidationIssue::Error(_)))
        .collect();

    assert!(
        errors.is_empty(),
        "family plan should have no validation errors, got: {errors:?}"
    );
}

#[test]
fn family_plan_json_roundtrip() {
    let toml_str = include_str!("fixtures/family-plan.toml");
    let plan = FrequencyPlan::from_toml(toml_str).unwrap();
    let json = plan.to_json().unwrap();
    let restored = FrequencyPlan::from_json(&json).unwrap();
    assert_eq!(plan, restored);
}

#[test]
fn family_plan_toml_roundtrip() {
    let toml_str = include_str!("fixtures/family-plan.toml");
    let plan = FrequencyPlan::from_toml(toml_str).unwrap();
    let re_toml = plan.to_toml().unwrap();
    let restored = FrequencyPlan::from_toml(&re_toml).unwrap();
    assert_eq!(plan, restored);
}
