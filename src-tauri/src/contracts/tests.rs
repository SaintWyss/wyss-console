use super::{
    Sequence,
    event_envelope::{EventEnvelopeV1, Persistence},
};
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/event-envelope-v1.json"
    ))
    .unwrap()
}

#[test]
fn shared_valid_fixtures_validate_and_round_trip() {
    for case in corpus()["valid"].as_array().unwrap() {
        let original = case["event"].clone();
        let event = EventEnvelopeV1::from_value(original.clone())
            .unwrap_or_else(|error| panic!("{}: {error}", case["name"]));
        event.validate().unwrap();
        let serialized = serde_json::to_value(&event).unwrap();
        assert_eq!(serialized, original, "{}", case["name"]);
        let restored: EventEnvelopeV1 = serde_json::from_value(serialized.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), serialized);
        assert!(serialized["sequence"].is_string());
    }
}

#[test]
fn shared_invalid_fixtures_reject_with_matching_diagnostics() {
    for case in corpus()["invalid"].as_array().unwrap() {
        let error = EventEnvelopeV1::from_value(case["event"].clone()).unwrap_err();
        assert_eq!(
            error.field,
            case["error"]["field"].as_str().unwrap(),
            "{}",
            case["name"]
        );
        assert_eq!(
            error.code.as_str(),
            case["error"]["code"].as_str().unwrap(),
            "{}",
            case["name"]
        );
        assert!(
            serde_json::from_value::<EventEnvelopeV1>(case["event"].clone()).is_err(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn maximum_sequence_and_ephemeral_default() {
    let maximum: Sequence = "18446744073709551615".parse().unwrap();
    assert_eq!(maximum.get(), u64::MAX);
    assert_eq!(
        serde_json::to_value(maximum).unwrap(),
        "18446744073709551615"
    );
    assert!(serde_json::from_value::<Sequence>(serde_json::json!(42)).is_err());
    assert_eq!(Persistence::default(), Persistence::Ephemeral);
    let event = EventEnvelopeV1::from_value(corpus()["valid"][0]["event"].clone()).unwrap();
    assert_eq!(event.sequence().get(), 1);
    assert_eq!(event.persistence(), Persistence::Ephemeral);
}
