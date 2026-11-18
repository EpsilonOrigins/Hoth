use hoth_config::{ClockMode, PublisherKind, ScenarioConfig, SelectionMode};

const EXAMPLE: &str = include_str!("../../../config/scenarios/example.yaml");
const EXTERNAL_CLOCK: &str = include_str!("fixtures/external-clock.yaml");
const DUPLICATE_PUBLISHERS: &str = include_str!("fixtures/duplicate-publishers.yaml");

#[test]
fn parses_the_existing_v1_example() {
    let scenario = ScenarioConfig::from_yaml(EXAMPLE).unwrap();
    assert_eq!(scenario.schema_version(), 1);
    assert_eq!(scenario.name, "example-stimulus");
    assert_eq!(scenario.catalog.snapshot, "2026-11");
    assert_eq!(
        scenario.time.start_utc.instant().to_rfc3339(),
        "2026-11-17T00:00:00+00:00"
    );
    assert_eq!(scenario.time.duration_seconds.get(), 600);
    assert_eq!(scenario.time.clock, ClockMode::Standalone);
    assert_eq!(scenario.time.rate, 1.0);
    assert_eq!(scenario.selection.mode, SelectionMode::All);
    assert_eq!(scenario.publishers.len(), 1);
    assert_eq!(scenario.publishers[0].kind, PublisherKind::Dis);
    assert_eq!(
        scenario.publishers[0].destination.to_string(),
        "127.0.0.1:3000"
    );
    assert_eq!(scenario.publishers[0].rate_hz, 25.0);
}

#[test]
fn parses_flow_mappings_and_normalizes_an_external_clock_start() {
    let external = ScenarioConfig::from_yaml(EXTERNAL_CLOCK).unwrap();
    let standalone = ScenarioConfig::from_yaml(EXAMPLE).unwrap();
    assert_eq!(external.time.clock, ClockMode::External);
    assert_eq!(external.time.start_utc, standalone.time.start_utc);
    assert_eq!(external.publishers.len(), 2);
    assert_eq!(external.publishers[0], standalone.publishers[0]);
    assert_eq!(external.publishers[1].destination.to_string(), "[::1]:3000");
    assert_eq!(external.publishers[1].rate_hz, 12.5);
}

#[test]
fn checks_the_schema_version_before_interpreting_v1_fields() {
    let input = EXAMPLE.replace("schema_version: 1", "schema_version: 2");
    let input = input.replace("duration_seconds: 600", "duration: a-new-format");
    assert_invalid(&input, "schema_version", "unsupported schema version 2");

    assert_invalid(
        &EXAMPLE.replace("schema_version: 1\n", ""),
        "schema_version",
        "missing field",
    );
    assert_invalid(
        &EXAMPLE.replace("schema_version: 1", "schema_version: '1'"),
        "schema_version",
        "expected i64",
    );
}

#[test]
fn rejects_invalid_utc_and_duration_with_field_paths() {
    for start in ["2026-02-30T00:00:00Z", "2026-11-17T00:00:00", "yesterday"] {
        assert_invalid(
            &EXAMPLE.replace("2026-11-17T00:00:00Z", start),
            "time.start_utc",
            "RFC 3339",
        );
    }
    for duration in ["-1", "0", "0.5"] {
        assert_invalid(
            &EXAMPLE.replace(
                "duration_seconds: 600",
                &format!("duration_seconds: {duration}"),
            ),
            "time.duration_seconds",
            "positive whole number of seconds",
        );
    }
}

#[test]
fn rejects_invalid_clock_and_publisher_rates_with_field_paths() {
    for rate in ["0", "-1", ".nan", ".inf", "-.inf"] {
        assert_invalid(
            &EXAMPLE.replace("rate: 1.0", &format!("rate: {rate}")),
            "time.rate",
            "finite and greater than zero",
        );
        assert_invalid(
            &EXAMPLE.replace("rate_hz: 25", &format!("rate_hz: {rate}")),
            "publishers[0].rate_hz",
            "finite and greater than zero",
        );
    }
}

#[test]
fn rejects_duplicate_publishers_by_kind_and_resolved_address() {
    assert_invalid(DUPLICATE_PUBLISHERS, "publishers[1]", "publishers[0]");
}

#[test]
fn rejects_invalid_names_snapshots_and_destinations() {
    for (field, replacement, path, reason) in [
        ("name: example-stimulus", "name: ' '", "name", "blank"),
        (
            "name: example-stimulus",
            "name: null",
            "name",
            "expected a string",
        ),
        ("\"2026-11\"", "\"2026-13\"", "catalog.snapshot", "YYYY-MM"),
        (
            "127.0.0.1:3000",
            "example.test:3000",
            "publishers[0].destination",
            "IP address",
        ),
        (
            "127.0.0.1:3000",
            "127.0.0.1:0",
            "publishers[0].destination",
            "greater than zero",
        ),
    ] {
        assert_invalid(&EXAMPLE.replace(field, replacement), path, reason);
    }
}

#[test]
fn rejects_unknown_modes_fields_and_empty_publishers() {
    for (field, replacement, path, reason) in [
        (
            "clock: standalone",
            "clock: realtime",
            "time.clock",
            "standalone",
        ),
        ("mode: all", "mode: radar_union", "selection.mode", "radars"),
        (
            "mode: all",
            "mode: all\n  radar_ids: []",
            "selection",
            "unknown field `radar_ids`",
        ),
        ("type: dis", "type: unknown", "publishers[0].type", "dis"),
        (
            "rate_hz: 25",
            "rates_hz: 25",
            "publishers[0]",
            "unknown field `rates_hz`",
        ),
    ] {
        assert_invalid(&EXAMPLE.replace(field, replacement), path, reason);
    }
    let (prefix, _) = EXAMPLE.split_once("publishers:").unwrap();
    assert_invalid(
        &format!("{prefix}publishers: []"),
        "publishers",
        "at least one",
    );
    assert!(ScenarioConfig::from_yaml(&format!("{EXAMPLE}radars: []")).is_ok());
}

#[test]
fn rejects_ambiguous_or_malformed_yaml() {
    assert_invalid(
        &EXAMPLE.replace("rate_hz: 25", "rate_hz: 25\n    rate_hz: 30"),
        "publishers[0]",
        "duplicate entry with key \"rate_hz\"",
    );
    assert_invalid(
        &format!("{EXAMPLE}---\n{EXAMPLE}"),
        "document",
        "more than one",
    );
    assert_invalid("schema_version: [1", "line", "column");
}

fn assert_invalid(input: &str, path: &str, reason: &str) {
    let error = ScenarioConfig::from_yaml(input).unwrap_err().to_string();
    assert!(
        error.contains(path) && error.contains(reason),
        "expected {path:?} and {reason:?} in error: {error}"
    );
}
