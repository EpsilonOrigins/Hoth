use hoth_catalog::{
    schema,
    sources::space_track::{
        coverage, Credentials, HistoryImporter, HistoryPlan, HistoryTransport, ImportError,
        MonthCoverage, PageQuery, Reply, TransportError,
    },
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::{self, File},
    path::PathBuf,
    time::{Duration, SystemTime},
};

const FIRST_PAGE: &[u8] = include_bytes!("fixtures/space_track/first-page.json");
const LAST_PAGE: &[u8] = include_bytes!("fixtures/space_track/last-page.json");

enum Step {
    Login(Reply),
    Fetch(&'static str, i64, Result<Reply, TransportError>),
}

struct MockHttp {
    steps: VecDeque<Step>,
}

impl HistoryTransport for &mut MockHttp {
    fn login(&mut self, _: &Credentials) -> Result<Reply, TransportError> {
        let Some(Step::Login(response)) = self.steps.pop_front() else {
            panic!("unexpected login request");
        };
        Ok(response)
    }

    fn fetch(&mut self, query: &PageQuery<'_>) -> Result<Reply, TransportError> {
        let Some(Step::Fetch(month, after, response)) = self.steps.pop_front() else {
            panic!("unexpected data request");
        };
        assert_eq!(query.object_scope, "25544,43013");
        assert_eq!(query.window.start.format("%Y-%m").to_string(), month);
        assert_eq!(query.after_gp_id, after);
        assert_eq!(query.limit, 2);
        response
    }
}

#[test]
fn honors_rate_limit_commits_pages_and_reports_missing_coverage() {
    let mut database = schema::open(":memory:").unwrap();
    let plan = plan("2020-04");
    let throttle = Reply {
        retry_after: Some(Duration::from_secs(42)),
        ..reply(429, b"")
    };
    let (result, delays) = run(
        &mut database,
        &plan,
        [
            login(),
            fetch("2020-02", 0, FIRST_PAGE),
            Step::Fetch("2020-02", 101, Ok(throttle)),
            fetch("2020-02", 101, LAST_PAGE),
            fetch("2020-03", 0, b"[]"),
        ],
    );
    assert_eq!(result.unwrap(), 3);
    assert_eq!(delays, [13, 13, 42, 13]);
    assert_eq!(
        coverage(&database, &plan).unwrap(),
        [
            MonthCoverage {
                month: "2020-02".into(),
                complete: true,
                missing_ids: vec![]
            },
            MonthCoverage {
                month: "2020-03".into(),
                complete: true,
                missing_ids: vec![25544, 43013]
            },
        ]
    );
    assert_eq!(count(&database, "catalog_acquisitions"), 2);
    assert_eq!(count(&database, "orbital_elements"), 3);

    let (raw, digest, epoch): (Vec<u8>, String, String) = database.query_row(
        "SELECT raw_record, checksum, epoch_utc FROM orbital_elements WHERE id = 'space-track:101'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    let records: Vec<&serde_json::value::RawValue> = serde_json::from_slice(FIRST_PAGE).unwrap();
    assert_eq!(raw, records[1].get().as_bytes());
    let expected: String = Sha256::digest(&raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(digest, expected);
    assert_eq!(epoch, "2020-02-29T23:59:59.999999000Z");
    let (reopen, delays) = run(&mut database, &plan, []);
    assert_eq!(reopen.unwrap(), 0);
    assert!(delays.is_empty());
}

#[test]
fn restarts_from_the_last_committed_page_after_bounded_5xx_and_timeout_retries() {
    let file = CatalogFile::new();
    let mut database = schema::open(&file.0).unwrap();
    let plan = plan("2020-03");
    let (first, delays) = run(
        &mut database,
        &plan,
        [
            login(),
            fetch("2020-02", 0, FIRST_PAGE),
            Step::Fetch("2020-02", 101, Ok(reply(503, b""))),
            Step::Fetch("2020-02", 101, Err(TransportError::Timeout)),
            Step::Fetch("2020-02", 101, Ok(reply(502, b""))),
            Step::Fetch("2020-02", 101, Ok(reply(503, b""))),
        ],
    );
    assert!(matches!(first, Err(ImportError::Http(503))));
    assert_eq!(delays, [13, 13, 15, 30, 60]);
    assert_eq!(count(&database, "orbital_elements"), 2);
    assert_eq!(
        coverage(&database, &plan).unwrap(),
        [MonthCoverage {
            month: "2020-02".into(),
            complete: false,
            missing_ids: vec![43013]
        },]
    );

    drop(database);
    let mut database = schema::open(&file.0).unwrap();
    let (resumed, _) = run(
        &mut database,
        &plan,
        [login(), fetch("2020-02", 101, LAST_PAGE)],
    );
    assert_eq!(resumed.unwrap(), 1);
    assert_eq!(count(&database, "orbital_elements"), 3);
    assert!(coverage(&database, &plan).unwrap()[0].complete);
}

#[test]
fn refreshes_an_expired_session_once_and_redacts_authentication_failures() {
    let mut database = schema::open(":memory:").unwrap();
    let plan = plan("2020-03");
    let unauthorized = || Step::Fetch("2020-02", 0, Ok(reply(401, b"test-only-password")));
    let (result, delays) = run(
        &mut database,
        &plan,
        [login(), unauthorized(), login(), unauthorized()],
    );
    assert_eq!(delays, [13, 13, 13]);
    let error = result.unwrap_err();
    assert!(matches!(error, ImportError::Authentication));
    assert!(!format!("{error} {error:?}").contains("test-only-password"));
    assert_eq!(count(&database, "space_track_checkpoints"), 0);

    let (result, _) = run(
        &mut database,
        &plan,
        [
            login(),
            unauthorized(),
            login(),
            fetch("2020-02", 0, LAST_PAGE),
        ],
    );
    assert_eq!(result.unwrap(), 1);
    assert_eq!(count(&database, "orbital_elements"), 1);
}

#[test]
fn refuses_login_failed_json_and_respects_long_retry_after_without_retrying() {
    let mut database = schema::open(":memory:").unwrap();
    let plan = plan("2020-03");
    let (failed, delays) = run(
        &mut database,
        &plan,
        [Step::Login(reply(
            200,
            br#"{"Login":"Failed","message":"test-only-password"}"#,
        ))],
    );
    let error = failed.unwrap_err();
    assert!(matches!(error, ImportError::Authentication));
    assert!(!format!("{error} {error:?}").contains("test-only-password"));
    assert!(delays.is_empty());

    let retry_after = Duration::from_secs(900);
    let (throttled, delays) = run(
        &mut database,
        &plan,
        [
            login(),
            Step::Fetch(
                "2020-02",
                0,
                Ok(Reply {
                    retry_after: Some(retry_after),
                    ..reply(429, b"")
                }),
            ),
        ],
    );
    assert!(matches!(throttled, Err(ImportError::RetryAfter(delay)) if delay == retry_after));
    assert_eq!(delays, [13]);
    assert_eq!(count(&database, "space_track_checkpoints"), 0);
}

#[test]
fn failed_page_and_checkpoint_roll_back_together_and_retry_from_the_same_cursor() {
    let mut database = schema::open(":memory:").unwrap();
    let plan = plan("2020-03");
    database
        .execute_batch(
            "CREATE TRIGGER reject_second BEFORE INSERT ON orbital_elements
         WHEN NEW.id = 'space-track:101' BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;",
        )
        .unwrap();
    let (failed, _) = run(
        &mut database,
        &plan,
        [login(), fetch("2020-02", 0, FIRST_PAGE)],
    );
    assert!(matches!(failed, Err(ImportError::Database(_))));
    for table in [
        "orbital_elements",
        "catalog_acquisitions",
        "space_objects",
        "space_track_checkpoints",
    ] {
        assert_eq!(count(&database, table), 0);
    }
    database
        .execute_batch("DROP TRIGGER reject_second")
        .unwrap();
    let (resumed, _) = run(
        &mut database,
        &plan,
        [
            login(),
            fetch("2020-02", 0, FIRST_PAGE),
            fetch("2020-02", 101, LAST_PAGE),
        ],
    );
    assert_eq!(resumed.unwrap(), 3);
}

#[test]
fn rejects_rows_outside_requested_scope_or_epoch_without_advancing() {
    let mut database = schema::open(":memory:").unwrap();
    let plan = plan("2020-03");
    for (old, new) in [("43013", "42"), ("2020-02-28", "2020-03-01"), ("102", "0")] {
        let bad = str::from_utf8(LAST_PAGE).unwrap().replace(old, new);
        let (result, _) = run(
            &mut database,
            &plan,
            [login(), fetch("2020-02", 0, bad.as_bytes())],
        );
        assert!(matches!(result, Err(ImportError::InvalidPage(_))));
        assert_eq!(count(&database, "orbital_elements"), 0);
        assert_eq!(count(&database, "space_track_checkpoints"), 0);
    }
}

fn plan(end: &str) -> HistoryPlan {
    HistoryPlan::new("2020-02", end, vec![43013, 25544]).unwrap()
}

fn run(
    database: &mut Connection,
    plan: &HistoryPlan,
    steps: impl IntoIterator<Item = Step>,
) -> (Result<usize, ImportError>, Vec<u64>) {
    let mut http = MockHttp {
        steps: steps.into_iter().collect(),
    };
    let mut delays = Vec::new();
    let credentials = Credentials::new("mock-user".into(), "test-only-password".into()).unwrap();
    let result = HistoryImporter::new(
        &mut http,
        |duration: Duration| delays.push(duration.as_secs()),
        credentials,
        2,
    )
    .unwrap()
    .run(database, plan);
    assert!(
        http.steps.is_empty(),
        "mock HTTP requests were not consumed"
    );
    (result, delays)
}

fn reply(status: u16, body: &[u8]) -> Reply {
    Reply {
        status,
        retry_after: None,
        body: body.to_vec(),
    }
}

fn login() -> Step {
    Step::Login(reply(200, br#""""#))
}

fn fetch(month: &'static str, after: i64, body: &[u8]) -> Step {
    Step::Fetch(month, after, Ok(reply(200, body)))
}

fn count(database: &Connection, table: &str) -> i64 {
    database
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

struct CatalogFile(PathBuf);

impl CatalogFile {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "hoth-space-track-{}-{unique}.sqlite",
            std::process::id()
        ));
        File::create_new(&path).unwrap();
        Self(path)
    }
}

impl Drop for CatalogFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
