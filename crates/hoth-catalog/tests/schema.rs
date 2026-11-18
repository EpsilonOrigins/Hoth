use hoth_catalog::schema::{self, SchemaError, SCHEMA_VERSION};
use rusqlite::{
    ffi::{
        SQLITE_CONSTRAINT_CHECK, SQLITE_CONSTRAINT_FOREIGNKEY, SQLITE_CONSTRAINT_PRIMARYKEY,
        SQLITE_CONSTRAINT_UNIQUE,
    },
    Connection,
};

const V1: &str = include_str!("../migrations/0001_catalog.sql");
const FIXTURE: &str = include_str!("fixtures/catalog.sql");

#[test]
fn fresh_catalog_enforces_foreign_keys_and_unique_membership() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    schema::migrate(&mut connection).unwrap();
    assert_eq!(pragma(&connection, "foreign_keys"), 1);
    assert_eq!(pragma(&connection, "user_version"), SCHEMA_VERSION);
    connection.execute_batch(FIXTURE).unwrap();

    assert_constraint(
        connection.execute(
            "INSERT INTO snapshot_members VALUES ('november', 43013, 'missing')",
            [],
        ),
        SQLITE_CONSTRAINT_FOREIGNKEY,
    );
    assert_constraint(
        connection.execute(
            "INSERT INTO snapshot_members VALUES ('november', 43013, 'early')",
            [],
        ),
        SQLITE_CONSTRAINT_FOREIGNKEY,
    );
    assert_constraint(
        connection.execute(
            "INSERT INTO snapshot_members VALUES ('missing', 25544, 'early')",
            [],
        ),
        SQLITE_CONSTRAINT_FOREIGNKEY,
    );
    assert_constraint(
        connection.execute(
            "INSERT INTO snapshot_members VALUES ('november', 25544, 'early')",
            [],
        ),
        SQLITE_CONSTRAINT_PRIMARYKEY,
    );
    assert_constraint(
        connection.execute("DELETE FROM orbital_elements WHERE id = 'late'", []),
        SQLITE_CONSTRAINT_FOREIGNKEY,
    );
    assert_constraint(
        connection.execute(
            "INSERT INTO catalog_snapshots VALUES ('same-month', '2026-11')",
            [],
        ),
        SQLITE_CONSTRAINT_UNIQUE,
    );
    assert_eq!(count(&connection, "snapshot_members"), 1);
}

#[test]
fn upgrades_a_populated_v1_catalog_and_preserves_source_data() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(V1).unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection.execute_batch(FIXTURE).unwrap();

    schema::migrate(&mut connection).unwrap();
    schema::migrate(&mut connection).unwrap();
    assert_eq!(pragma(&connection, "user_version"), SCHEMA_VERSION);
    assert_eq!(count(&connection, "orbital_elements"), 2);
    assert_eq!(count(&connection, "snapshot_members"), 1);

    let (source_epoch, utc, raw, source, rcs): (String, String, Vec<u8>, String, Option<f64>) =
        connection
            .query_row(
                "SELECT e.source_epoch, e.epoch_utc, e.raw_record, a.source, o.rcs_m2
             FROM snapshot_members m
             JOIN orbital_elements e ON e.norad_id = m.norad_id AND e.id = m.element_id
             JOIN catalog_acquisitions a ON a.id = e.acquisition_id
             JOIN space_objects o ON o.norad_id = m.norad_id
             WHERE m.snapshot_id = 'november' AND m.norad_id = 25544",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
    assert_eq!(source_epoch, "2026-11-16T23:59:59.098765432");
    assert_eq!(utc, "2026-11-16T23:59:59.098765432Z");
    assert_eq!(raw, br#"{"NORAD_CAT_ID":25544,"EPOCH":"2026-11-16T23:59:59.098765432","MEAN_MOTION":15.5,"ECCENTRICITY":0.0002,"INCLINATION":51.6,"RA_OF_ASC_NODE":120.0,"ARG_OF_PERICENTER":30.0,"MEAN_ANOMALY":10.0,"BSTAR":0.00001}"#);
    assert_eq!(source, "fixture-elements");
    assert_eq!(rcs, None);
    let first: String = connection
        .query_row(
            "SELECT id FROM orbital_elements WHERE norad_id = 25544 ORDER BY epoch_utc LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(first, "early");
}

#[test]
fn indexes_cover_monthly_membership_and_epoch_queries() {
    let connection = schema::open(":memory:").unwrap();
    assert_index(
        &connection,
        "SELECT element_id FROM snapshot_members WHERE snapshot_id = 'november' ORDER BY norad_id",
        "PRIMARY KEY",
    );
    assert_index(
        &connection,
        "SELECT id FROM orbital_elements WHERE norad_id = 25544 ORDER BY epoch_utc",
        "orbital_elements_norad_epoch",
    );
    assert_index(
        &connection,
        "SELECT id FROM orbital_elements WHERE epoch_utc >= '2026-11-01T00:00:00.000000000Z'",
        "orbital_elements_epoch",
    );
    assert_index(
        &connection,
        "SELECT snapshot_id FROM snapshot_members WHERE norad_id = 25544 AND element_id = 'late'",
        "snapshot_members_element",
    );
}

#[test]
fn rolls_back_all_pending_migrations_when_an_index_creation_fails() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE blocker (id INTEGER);
         CREATE INDEX orbital_elements_epoch ON blocker (id);",
        )
        .unwrap();

    assert!(matches!(
        schema::migrate(&mut connection),
        Err(SchemaError::Migration { version: 2, .. })
    ));
    assert!(connection.is_autocommit());
    assert_eq!(pragma(&connection, "user_version"), 0);
    let catalog_objects: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name IN
         ('catalog_acquisitions', 'space_objects', 'orbital_elements', 'catalog_snapshots',
          'snapshot_members', 'orbital_elements_norad_epoch')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(catalog_objects, 0);
    connection.execute("DROP TABLE blocker", []).unwrap();
    schema::migrate(&mut connection).unwrap();
    assert_eq!(pragma(&connection, "user_version"), SCHEMA_VERSION);
}

#[test]
fn refuses_future_versions_and_callers_active_transactions() {
    let mut connection = schema::open(":memory:").unwrap();
    connection.execute_batch("BEGIN").unwrap();
    assert!(matches!(
        schema::migrate(&mut connection),
        Err(SchemaError::TransactionActive)
    ));
    assert!(!connection.is_autocommit());
    connection.execute_batch("ROLLBACK").unwrap();

    let future = SCHEMA_VERSION + 1;
    connection
        .pragma_update(None, "user_version", future)
        .unwrap();
    assert!(matches!(
        schema::migrate(&mut connection),
        Err(SchemaError::UnsupportedVersion { found }) if found == future
    ));
    assert_eq!(pragma(&connection, "user_version"), future);
}

#[test]
fn failed_batch_can_roll_back_the_snapshot_and_all_prior_members() {
    let mut connection = schema::open(":memory:").unwrap();
    connection.execute_batch(FIXTURE).unwrap();
    {
        let transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "INSERT INTO catalog_snapshots VALUES ('december', '2026-12')",
                [],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO snapshot_members VALUES ('december', 25544, 'late')",
                [],
            )
            .unwrap();
        assert_constraint(
            transaction.execute(
                "INSERT INTO snapshot_members VALUES ('december', 43013, 'missing')",
                [],
            ),
            SQLITE_CONSTRAINT_FOREIGNKEY,
        );
        transaction.rollback().unwrap();
    }
    assert_eq!(count(&connection, "catalog_snapshots"), 1);
    assert_eq!(count(&connection, "snapshot_members"), 1);

    connection
        .execute("DELETE FROM catalog_snapshots WHERE id = 'november'", [])
        .unwrap();
    assert_eq!(count(&connection, "snapshot_members"), 0);
    assert_eq!(count(&connection, "orbital_elements"), 2);
}

#[test]
fn rejects_bad_record_metadata_and_duplicate_payloads() {
    let connection = schema::open(":memory:").unwrap();
    connection.execute_batch(FIXTURE).unwrap();
    for sql in [
        "UPDATE orbital_elements SET format = 'csv'",
        "UPDATE orbital_elements SET epoch_utc = '2026-11-16T23:59:59Z'",
        "UPDATE orbital_elements SET checksum = 'bad'",
        "UPDATE space_objects SET rcs_m2 = 1 WHERE norad_id = 43013",
        "INSERT INTO catalog_snapshots VALUES ('bad-month', '2026-13')",
    ] {
        assert_constraint(connection.execute(sql, []), SQLITE_CONSTRAINT_CHECK);
    }
    assert_constraint(
        connection.execute(
            "INSERT INTO orbital_elements
             SELECT 'duplicate', norad_id, epoch_utc, source_epoch, format,
                    raw_record, checksum, acquisition_id
             FROM orbital_elements WHERE id = 'early'",
            [],
        ),
        SQLITE_CONSTRAINT_UNIQUE,
    );
}

fn assert_constraint(result: rusqlite::Result<usize>, expected: i32) {
    let error = result.unwrap_err();
    assert_eq!(
        error.sqlite_error().unwrap().extended_code,
        expected,
        "{error}"
    );
}

fn assert_index(connection: &Connection, sql: &str, index: &str) {
    let detail: String = connection
        .query_row(&format!("EXPLAIN QUERY PLAN {sql}"), [], |row| row.get(3))
        .unwrap();
    assert!(
        detail.contains(index),
        "expected {index} in query plan: {detail}"
    );
}

fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn pragma(connection: &Connection, name: &str) -> i64 {
    connection
        .pragma_query_value(None, name, |row| row.get(0))
        .unwrap()
}
