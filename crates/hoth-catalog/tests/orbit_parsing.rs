use chrono::SecondsFormat;
use hoth_catalog::{
    parse::{parse_omm_csv, parse_omm_json, parse_tle, ParseError, ParseWarning},
    schema,
};
use hoth_domain::orbit::{ElementFormat, RecordProvenance};
use rusqlite::params;
use serde_json::{json, Value};

const TLE: &[u8] = include_bytes!("fixtures/orbits/iss.tle");
const OMM_JSON: &[u8] = include_bytes!("fixtures/orbits/iss.omm.json");
const OMM_CSV: &[u8] = include_bytes!("fixtures/orbits/iss.omm.csv");
const UTC_EPOCH: &str = "2008-09-20T12:25:40.104192000Z";

#[test]
fn normalizes_equivalent_tle_json_and_csv() {
    let tle = parse_tle(TLE, provenance()).unwrap();
    let json = parse_omm_json(OMM_JSON, provenance()).unwrap();
    let csv = parse_omm_csv(OMM_CSV, provenance()).unwrap();

    for parsed in [&tle, &json, &csv] {
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.element.norad_id.get(), 25544);
        assert_eq!(
            parsed
                .element
                .epoch_utc
                .to_rfc3339_opts(SecondsFormat::Nanos, true),
            UTC_EPOCH
        );
        let orbit = &parsed.element.mean_elements;
        assert_eq!(orbit.inclination_deg, 51.6416);
        assert_eq!(orbit.right_ascension_deg, 247.4627);
        assert_eq!(orbit.eccentricity, 0.0006703);
        assert_eq!(orbit.argument_of_perigee_deg, 130.536);
        assert_eq!(orbit.mean_anomaly_deg, 325.0288);
        assert_eq!(orbit.mean_motion_rev_day, 15.72125391);
        assert_eq!(orbit.mean_motion_dot, Some(-0.00002182));
        assert_eq!(orbit.mean_motion_ddot, Some(0.0));
        assert!((orbit.bstar.unwrap() - (-0.000011606)).abs() < 1e-18);
    }
    assert_eq!(json.element.mean_elements, csv.element.mean_elements);
}

#[test]
fn keeps_original_formats_epoch_text_and_byte_checksums() {
    for (parsed, format, epoch, bytes, digest) in [
        (
            parse_tle(TLE, provenance()).unwrap(),
            ElementFormat::Tle,
            "08264.51782528",
            TLE,
            "b4377fcb077c5f892b4f8cbfbbd1a3e93c89c3e0561d22d7cfb334d1169fe40f",
        ),
        (
            parse_omm_json(OMM_JSON, provenance()).unwrap(),
            ElementFormat::Omm,
            "2008-09-20T12:25:40.104192Z",
            OMM_JSON,
            "4848bbc5eb3fb27e42f75a33aca8213109ef4cfab573ef2fb3a7ebd0e21fc13b",
        ),
        (
            parse_omm_csv(OMM_CSV, provenance()).unwrap(),
            ElementFormat::Omm,
            "2008-09-20T12:25:40.104192",
            OMM_CSV,
            "b308bee977f8617725acdce8360bb95189174e9c0425e93934500b82ac20b7ef",
        ),
    ] {
        assert_eq!(parsed.element.raw_source.format, format);
        assert_eq!(parsed.element.raw_source.epoch_text, epoch);
        assert_eq!(parsed.element.raw_source.record, bytes);
        assert_eq!(parsed.element.raw_source.checksum, digest);
        assert_eq!(parsed.element.raw_source.provenance, provenance());
    }
    let crlf = str::from_utf8(TLE).unwrap().replace('\n', "\r\n");
    let windows = parse_tle(crlf.as_bytes(), provenance()).unwrap().element;
    assert_eq!(windows.raw_source.record, crlf.as_bytes());
    assert_ne!(
        windows.raw_source.checksum,
        parse_tle(TLE, provenance())
            .unwrap()
            .element
            .raw_source
            .checksum
    );
}

#[test]
fn rejects_tle_checksum_errors_and_non_digit_checksums() {
    for (line, expected) in [(1, b'7'), (2, b'7')] {
        let invalid = edit_tle(line, 68, b"3", false);
        assert!(matches!(
            parse_tle(&invalid, provenance()),
            Err(ParseError::TleChecksum { line: bad, expected: digit, actual: 3 })
                if bad == line as u8 && digit == expected - b'0'
        ));
        let invalid = edit_tle(line, 68, b"/", false);
        assert!(matches!(
            parse_tle(&invalid, provenance()),
            Err(ParseError::TleLine { line: bad, .. }) if bad == line as u8
        ));
    }
}

#[test]
fn rejects_mismatched_norad_malformed_tle_fields_and_bad_lines() {
    let mismatch = edit_tle(2, 2, b"43013", true);
    assert!(matches!(
        parse_tle(&mismatch, provenance()),
        Err(ParseError::TleNoradMismatch { line1, line2 })
            if line1.get() == 25544 && line2.get() == 43013
    ));
    for (line, offset, data, field) in [
        (1, 18, &b"09366.00000000"[..], "EPOCH"),
        (1, 18, &b"08000.00000000"[..], "EPOCH"),
        (1, 53, &b"-11606A4"[..], "BSTAR"),
        (2, 26, &b"00A6703"[..], "ECCENTRICITY"),
        (2, 52, &b"00.00000000"[..], "MEAN_MOTION"),
    ] {
        let malformed = edit_tle(line, offset, data, true);
        assert_invalid_field(parse_tle(&malformed, provenance()), field);
    }
    let mut extra = TLE.to_vec();
    extra.extend_from_slice(b"another line\n");
    assert!(matches!(
        parse_tle(&extra, provenance()),
        Err(ParseError::TleRecordCount)
    ));
    let truncated = &TLE[..TLE.len() - 2];
    assert!(matches!(
        parse_tle(truncated, provenance()),
        Err(ParseError::TleLine { line: 2, .. })
    ));
}

#[test]
fn parses_alpha5_leap_year_epochs_and_missing_drag_terms() {
    let alpha5 = edit_tle(1, 2, b"T2345", true);
    let alpha5 = edit_line(&alpha5, 2, 2, b"T2345", true);
    let old_epoch = edit_line(&alpha5, 1, 18, b"57365.99999999", true);
    let old_record = parse_tle(&old_epoch, provenance()).unwrap().element;
    assert_eq!(old_record.norad_id.get(), 272345);
    assert_eq!(
        old_record
            .epoch_utc
            .to_rfc3339_opts(SecondsFormat::Nanos, true),
        "1957-12-31T23:59:59.999136000Z"
    );

    let leap = edit_tle(1, 18, b"24366.00000001", true);
    let no_bstar = edit_line(&leap, 1, 53, b"        ", true);
    let parsed = parse_tle(&no_bstar, provenance()).unwrap();
    assert_eq!(
        parsed
            .element
            .epoch_utc
            .to_rfc3339_opts(SecondsFormat::Nanos, true),
        "2024-12-31T00:00:00.000864000Z"
    );
    assert_eq!(parsed.element.mean_elements.bstar, None);
    assert_eq!(parsed.warnings, [ParseWarning::MissingDragField("BSTAR")]);
}

#[test]
fn normalizes_omm_offsets_numeric_strings_and_reported_defaults() {
    let mut omm: Value = serde_json::from_slice(OMM_JSON).unwrap();
    let fields = omm.as_object_mut().unwrap();
    fields.insert("EPOCH".into(), json!("2008-09-20T07:25:40.104192-05:00"));
    fields.insert("NORAD_CAT_ID".into(), json!("25544"));
    fields.insert("MEAN_MOTION".into(), json!("15.72125391"));
    fields.insert("MEAN_MOTION_DOT".into(), Value::Null);
    fields.remove("MEAN_MOTION_DDOT");
    fields.insert("BSTAR".into(), json!(" "));
    fields.remove("CENTER_NAME");
    fields.remove("REF_FRAME");
    fields.remove("TIME_SYSTEM");
    fields.remove("MEAN_ELEMENT_THEORY");
    let parsed = parse_omm_json(&serde_json::to_vec(&omm).unwrap(), provenance()).unwrap();
    assert_eq!(
        parsed
            .element
            .epoch_utc
            .to_rfc3339_opts(SecondsFormat::Nanos, true),
        UTC_EPOCH
    );
    assert_eq!(parsed.element.mean_elements.bstar, None);
    assert_eq!(parsed.element.mean_elements.mean_motion_dot, None);
    assert_eq!(parsed.element.mean_elements.mean_motion_ddot, None);
    assert_eq!(parsed.warnings.len(), 7);
    assert!(parsed.warnings.contains(&ParseWarning::AssumedOmmMetadata {
        field: "REF_FRAME",
        value: "TEME",
    }));
    assert!(parsed
        .warnings
        .contains(&ParseWarning::MissingDragField("BSTAR")));
}

#[test]
fn rejects_missing_and_malformed_omm_fields() {
    let mut missing: Value = serde_json::from_slice(OMM_JSON).unwrap();
    missing.as_object_mut().unwrap().remove("ECCENTRICITY");
    let error = parse_omm_json(&serde_json::to_vec(&missing).unwrap(), provenance()).unwrap_err();
    assert!(error.to_string().contains("missing field `ECCENTRICITY`"));

    for (field, value) in [
        ("EPOCH", json!("2008-02-30T12:25:40Z")),
        ("EPOCH", json!("2008-09-20T12:25:40.1234567891Z")),
        ("ECCENTRICITY", json!(1)),
        ("ECCENTRICITY", json!("NaN")),
        ("MEAN_MOTION", json!(0)),
        ("INCLINATION", json!(181)),
        ("NORAD_CAT_ID", json!("4294967296")),
        ("TIME_SYSTEM", json!("TAI")),
        ("CENTER_NAME", json!("MARS")),
        ("REF_FRAME", json!("ITRF")),
        ("MEAN_ELEMENT_THEORY", json!("DSST")),
    ] {
        let mut invalid: Value = serde_json::from_slice(OMM_JSON).unwrap();
        invalid[field] = value;
        assert_invalid_field(
            parse_omm_json(&serde_json::to_vec(&invalid).unwrap(), provenance()),
            field,
        );
    }
    let text = str::from_utf8(OMM_CSV).unwrap();
    let missing = text.replacen(",0.0006703,", ",,", 1);
    assert_invalid_field(
        parse_omm_csv(missing.as_bytes(), provenance()),
        "ECCENTRICITY",
    );
    let missing_header = text.replacen(",ECCENTRICITY,", ",UNKNOWN,", 1);
    let error = parse_omm_csv(missing_header.as_bytes(), provenance()).unwrap_err();
    assert!(
        error.to_string().contains("missing field `ECCENTRICITY`"),
        "{error}"
    );
    let (_, row) = text.split_once('\n').unwrap();
    let multiple = format!("{text}{row}");
    assert!(matches!(
        parse_omm_csv(multiple.as_bytes(), provenance()),
        Err(ParseError::OmmCsvRecordCount)
    ));
    assert_invalid_field(parse_omm_json(b"[]", provenance()), "OMM JSON");
    let blank_provenance = RecordProvenance {
        source: " ".into(),
        ..provenance()
    };
    assert!(matches!(
        parse_tle(TLE, blank_provenance),
        Err(ParseError::InvalidProvenance)
    ));
}

#[test]
fn parsed_record_matches_the_sqlite_catalog_contract() {
    let element = parse_tle(TLE, provenance()).unwrap().element;
    let database = schema::open(":memory:").unwrap();
    let raw = &element.raw_source;
    let epoch = element
        .epoch_utc
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
    database
        .execute(
            "INSERT INTO catalog_acquisitions (id, source, acquired_at_utc) VALUES (?1, ?2, ?3)",
            params![
                "acquisition",
                raw.provenance.source,
                "2026-11-18T00:00:00.000000000Z"
            ],
        )
        .unwrap();
    database
        .execute(
            "INSERT INTO space_objects (norad_id) VALUES (?1)",
            [element.norad_id.get()],
        )
        .unwrap();
    database
        .execute(
            "INSERT INTO orbital_elements
                (id, norad_id, epoch_utc, source_epoch, format, raw_record, checksum, acquisition_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'acquisition')",
            params![
                raw.provenance.record_id, element.norad_id.get(), epoch, raw.epoch_text,
                raw.format.as_str(), raw.record, raw.checksum
            ],
        )
        .unwrap();
    let stored: (String, String, Vec<u8>, String) = database
        .query_row(
            "SELECT epoch_utc, source_epoch, raw_record, checksum FROM orbital_elements",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        stored,
        (
            UTC_EPOCH.into(),
            "08264.51782528".into(),
            TLE.to_vec(),
            raw.checksum.clone()
        )
    );
}

fn provenance() -> RecordProvenance {
    RecordProvenance {
        source: "local-fixture".into(),
        record_id: "iss-2008-264".into(),
    }
}

fn assert_invalid_field(
    result: Result<hoth_catalog::parse::ParsedElement, ParseError>,
    field: &str,
) {
    let error = result.unwrap_err();
    assert!(
        matches!(error, ParseError::InvalidField { field: name, .. } if name == field),
        "{error}"
    );
}

fn edit_tle(line: usize, offset: usize, replacement: &[u8], repair_checksum: bool) -> Vec<u8> {
    edit_line(TLE, line, offset, replacement, repair_checksum)
}

fn edit_line(
    payload: &[u8],
    line: usize,
    offset: usize,
    replacement: &[u8],
    repair_checksum: bool,
) -> Vec<u8> {
    let mut lines: Vec<_> = payload
        .split(|&byte| byte == b'\n')
        .map(|line| line.to_vec())
        .collect();
    let target = &mut lines[line];
    target[offset..offset + replacement.len()].copy_from_slice(replacement);
    if repair_checksum {
        let checksum = target[..68].iter().fold(0u16, |total, &byte| {
            total
                + if byte.is_ascii_digit() {
                    u16::from(byte - b'0')
                } else {
                    u16::from(byte == b'-')
                }
        });
        target[68] = b'0' + (checksum % 10) as u8;
    }
    lines.join(&b'\n')
}
