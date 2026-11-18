use hoth_catalog::{offline, schema};
use serde_json::json;

const INPUT: &str = include_str!("fixtures/orbits/iss.omm.json");
const AT: &str = "2026-11-18T00:00:00Z";

#[test]
fn deterministic_monthly_snapshots_and_portable_export() {
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    schema::migrate(&mut db).unwrap();
    let base: serde_json::Value = serde_json::from_str(INPUT).unwrap();
    let row = |norad, epoch| {
        let mut data = base.clone();
        data["NORAD_CAT_ID"] = json!(norad);
        data["EPOCH"] = json!(epoch);
        data
    };
    let input = json!([
        row(9, "2026-09-13T00:00:00Z"),
        row(9, "2026-09-29T00:00:00Z"),
        row(9, "2026-09-13T00:00:00Z"),
        row(10, "2026-10-12T00:00:00Z"),
        row(11, "2026-10-16T00:00:00.000000001Z"),
        row(11, "2026-10-14T00:00:00.000000001Z"),
    ]);
    let bytes = serde_json::to_vec(&input).unwrap();
    assert_eq!(
        offline::import_elements(&mut db, "omm-json", &bytes, "fixture", AT).unwrap(),
        5
    );
    assert_eq!(
        offline::import_elements(&mut db, "omm-json", &bytes, "fixture", AT).unwrap(),
        0
    );
    for (month, count) in [("2026-09", 1), ("2026-10", 2), ("2026-11", 0)] {
        assert_eq!(offline::materialize(&mut db, month, 5).unwrap(), count);
    }
    let (_, september) = offline::snapshot(&db, "2026-09").unwrap();
    assert_eq!(september[0].raw_source.epoch_text, "2026-09-13T00:00:00Z");
    assert_eq!(
        offline::snapshot(&db, "2026-10").unwrap().1[1]
            .raw_source
            .epoch_text,
        "2026-10-14T00:00:00.000000001Z"
    );
    assert!(offline::snapshot(&db, "2026-11").is_err());
    let folder = tempfile::tempdir().unwrap();
    let export = folder.path().join("export.sqlite");
    offline::export(&db, &export).unwrap();
    assert!(offline::export(&db, &export).is_err());
    let imported = offline::open_read_only(&export).unwrap();
    assert_eq!(offline::validate(&imported).unwrap(), 5);
    assert_eq!(
        offline::snapshot(&imported, "2026-09").unwrap().1,
        september
    );
    assert_eq!(
        offline::stats(&imported).unwrap(),
        [
            ("2026-09".into(), 1),
            ("2026-10".into(), 2),
            ("2026-11".into(), 0)
        ]
    );
    db.execute("UPDATE orbital_elements SET checksum = lower(hex(zeroblob(32))) WHERE norad_id = 9 AND source_epoch LIKE '%13%'", []).unwrap();
    assert!(offline::validate(&db).is_err());
}

#[test]
fn retains_numeric_rcs_across_newer_unknown_metadata() {
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    schema::migrate(&mut db).unwrap();
    offline::import_metadata(
        &mut db,
        br#"[{"NORAD_CAT_ID":"9","OBJECT_NAME":"older","RCS":"2.5"}]"#,
        "SATCAT",
        "2026-11-16T00:00:00Z",
    )
    .unwrap();
    offline::import_metadata(
        &mut db,
        br#"[{"norad_id":9,"name":"newer","rcs_m2":null},{"norad_id":10}]"#,
        "other",
        AT,
    )
    .unwrap();
    let result: (String, f64, String, String) = db
        .query_row(
            "SELECT s.name, s.rcs_m2, a.source, b.source FROM space_objects s
         JOIN catalog_acquisitions a ON a.id=s.metadata_acquisition_id
         JOIN catalog_acquisitions b ON b.id=s.rcs_acquisition_id WHERE s.norad_id=9",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        result,
        ("newer".into(), 2.5, "other".into(), "SATCAT".into())
    );
    assert!(
        offline::import_metadata(&mut db, br#"[{"norad_id":11},{"norad_id":11}]"#, "dup", AT)
            .is_err()
    );
    let count: i64 = db
        .query_row("SELECT count(*) FROM space_objects", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn bulk_csv_and_tle_import_keep_per_record_source_bytes() {
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    schema::migrate(&mut db).unwrap();
    let csv = include_str!("fixtures/orbits/iss.omm.csv");
    let (header, record) = csv.split_once('\n').unwrap();
    let second = record.replace("25544", "70001");
    let multi = format!("{header}\n{record}{second}");
    assert_eq!(
        offline::import_elements(&mut db, "omm-csv", multi.as_bytes(), "csv", AT).unwrap(),
        2
    );
    let tle = include_bytes!("fixtures/orbits/iss.tle");
    assert_eq!(
        offline::import_elements(
            &mut db,
            "tle",
            &[tle.as_slice(), tle.as_slice()].concat(),
            "tle",
            AT
        )
        .unwrap(),
        1
    );
    assert_eq!(offline::validate(&db).unwrap(), 3);
    let original: Vec<u8> = db
        .query_row(
            "SELECT raw_record FROM orbital_elements WHERE norad_id=70001",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(original, format!("{header}\n{second}").into_bytes());
}

#[test]
fn sixty_month_archive_materializes_each_completed_month() {
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    schema::migrate(&mut db).unwrap();
    let months = offline::months("2021-11", "2026-11").unwrap();
    assert_eq!(months.len(), 60);
    assert_eq!(months.last().unwrap(), "2026-10");
    let base: serde_json::Value = serde_json::from_str(INPUT).unwrap();
    let rows: Vec<_> = months
        .iter()
        .map(|month| {
            let mut row = base.clone();
            row["EPOCH"] = json!(format!("{month}-15T00:00:00Z"));
            row
        })
        .collect();
    offline::import_elements(
        &mut db,
        "omm-json",
        &serde_json::to_vec(&rows).unwrap(),
        "archive",
        AT,
    )
    .unwrap();
    for month in &months {
        assert_eq!(offline::materialize(&mut db, month, 17).unwrap(), 1);
    }
    assert_eq!(offline::stats(&db).unwrap().len(), 60);
    assert_eq!(offline::validate(&db).unwrap(), 60);
}
