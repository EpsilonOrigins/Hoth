use hoth_catalog::{
    schema,
    sources::space_track::{
        coverage, Credentials, HistoryImporter, HistoryPlan, SpaceTrackHttp, DEFAULT_PAGE_SIZE,
    },
};
use std::{error::Error, ffi::OsString, fs, path::PathBuf, process::ExitCode, thread};

const USAGE: &str =
    "Usage: hoth catalog bootstrap (--start YYYY-MM --end YYYY-MM | --years 5) [--norad ID,ID] [--database PATH] [--dry-run] [--interval monthly]";

pub fn bootstrap(args: &[OsString]) -> ExitCode {
    if matches!(args, [flag] if flag == "-h" || flag == "--help") {
        println!("{USAGE}\n\nThe end month is exclusive. Live imports require --norad and Space-Track credentials in the environment.");
        return ExitCode::SUCCESS;
    }
    match Options::parse(args) {
        Ok(options) => match options.run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("error: {error}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

struct Options {
    plan: HistoryPlan,
    database: PathBuf,
    dry_run: bool,
}

impl Options {
    fn parse(args: &[OsString]) -> Result<Self, String> {
        let mut args = args.iter();
        let (mut start, mut end, mut ids, mut database) = (None, None, None, None);
        let (mut years, mut interval) = (None, None);
        let mut dry_run = false;
        while let Some(flag) = args.next() {
            match flag.to_str() {
                Some("--dry-run") if !dry_run => dry_run = true,
                Some("--start") if start.is_none() => {
                    start = args.next().and_then(|value| value.to_str())
                }
                Some("--end") if end.is_none() => {
                    end = args.next().and_then(|value| value.to_str())
                }
                Some("--years") if years.is_none() => {
                    years = Some(
                        args.next()
                            .and_then(|value| value.to_str())
                            .ok_or("provide the number of years")?
                            .parse::<u32>()
                            .map_err(|_| "years must be 1 through 5")?,
                    );
                }
                Some("--interval") if interval.is_none() => {
                    interval = Some(
                        args.next()
                            .and_then(|value| value.to_str())
                            .filter(|value| *value == "monthly")
                            .ok_or("interval must be monthly")?,
                    );
                }
                Some("--norad") if ids.is_none() => {
                    let value = args
                        .next()
                        .and_then(|value| value.to_str())
                        .ok_or("provide a comma-separated list for --norad")?;
                    ids = Some(
                        value
                            .split(',')
                            .map(str::parse::<u32>)
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|_| {
                                "provide a comma-separated list of positive integers for --norad"
                            })?,
                    );
                }
                Some("--database") if database.is_none() => {
                    database = Some(PathBuf::from(
                        args.next().ok_or("provide a path for --database")?,
                    ));
                }
                _ => return Err("unknown or repeated catalog bootstrap option".to_owned()),
            }
        }
        let rolling;
        if let Some(years) = years {
            use chrono::{Datelike, Months, Utc};
            if start.is_some() || end.is_some() || !(1..=5).contains(&years) {
                return Err("use --years 1–5 by itself or supply --start and --end".into());
            }
            let end_date = chrono::DateTime::<Utc>::from(std::time::SystemTime::now())
                .date_naive()
                .with_day(1)
                .expect("first of month");
            let start_date = end_date - Months::new(years * 12);
            rolling = (
                start_date.format("%Y-%m").to_string(),
                end_date.format("%Y-%m").to_string(),
            );
            start = Some(&rolling.0);
            end = Some(&rolling.1);
        }
        let plan = HistoryPlan::new(
            start.ok_or("provide --start")?,
            end.ok_or("provide --end")?,
            ids.unwrap_or_default(),
        )
        .map_err(|error| error.to_string())?;
        if !dry_run {
            plan.validate_live().map_err(|error| error.to_string())?;
        }
        Ok(Self {
            plan,
            database: database.unwrap_or_else(|| PathBuf::from("data/hoth.db")),
            dry_run,
        })
    }

    fn run(self) -> Result<(), Box<dyn Error>> {
        if self.dry_run {
            println!(
                "Space-Track GP History preview: {} through {} (end exclusive), {} explicit NORAD IDs. No requests made.",
                self.plan.start().format("%Y-%m"),
                self.plan.end().format("%Y-%m"),
                self.plan.ids().len()
            );
            return Ok(());
        }
        let credentials = Credentials::from_env()?;
        create_database_parent(&self.database)?;
        let mut database = schema::open(&self.database)?;
        let mut importer = HistoryImporter::new(
            SpaceTrackHttp::new(),
            thread::sleep,
            credentials,
            DEFAULT_PAGE_SIZE,
        )?;
        let imported = importer.run(&mut database, &self.plan);
        let report = coverage(&database, &self.plan);
        if let Ok(report) = &report {
            for month in report {
                let state = if month.complete {
                    "complete"
                } else {
                    "incomplete"
                };
                if month.missing_ids.is_empty() {
                    println!("{}: {state}", month.month);
                } else {
                    println!(
                        "{}: {state}, NORAD IDs without records so far: {:?}",
                        month.month, month.missing_ids
                    );
                }
            }
        }
        let count = imported?;
        report?;
        let months = hoth_catalog::offline::months(
            &self.plan.start().format("%Y-%m").to_string(),
            &self.plan.end().format("%Y-%m").to_string(),
        )?;
        materialize_months(&mut database, months, 17)?;
        println!("Imported {count} new Space-Track orbital records.");
        Ok(())
    }
}

/// Explicit local acquisition and snapshot operations. Runtime never calls these.
pub fn offline(command: &str, args: &[OsString]) -> Result<(), Box<dyn Error>> {
    use hoth_catalog::offline;
    use std::collections::BTreeMap;
    let allowed: &[&str] = match command {
        "import" => &[
            "--file",
            "--format",
            "--source",
            "--acquired-at",
            "--database",
        ],
        "metadata" => &["--file", "--source", "--acquired-at", "--database"],
        "snapshot" => &[
            "--month",
            "--start",
            "--end",
            "--max-age-days",
            "--database",
        ],
        "validate" | "stats" => &["--database", "--start", "--end"],
        "export" => &["--output", "--database"],
        _ => return Err(format!("unknown catalog operation {command}").into()),
    };
    let mut options = BTreeMap::new();
    let (pairs, remainder) = args.as_chunks::<2>();
    for pair in pairs {
        let flag = pair[0].to_str().ok_or("invalid option")?;
        if !allowed.contains(&flag) || options.insert(flag, &pair[1]).is_some() {
            return Err(format!("unknown or repeated option {flag}").into());
        }
    }
    if !remainder.is_empty() {
        return Err("option requires a value".into());
    }
    let text = |option: &str| options.get(option).and_then(|arg| arg.to_str());
    let file = |option: &str| {
        options
            .get(option)
            .map(PathBuf::from)
            .ok_or_else(|| format!("provide {option}"))
    };
    let database = options
        .get("--database")
        .map_or_else(|| PathBuf::from("data/hoth.db"), PathBuf::from);
    match command {
        "import" | "metadata" => {
            let bytes = fs::read(file("--file")?)?;
            let source = text("--source").unwrap_or("offline");
            let now =
                chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339();
            let at = text("--acquired-at").unwrap_or(&now);
            create_database_parent(&database)?;
            let mut db = schema::open(&database)?;
            let count = if command == "import" {
                offline::import_elements(
                    &mut db,
                    text("--format").ok_or("provide --format")?,
                    &bytes,
                    source,
                    at,
                )?
            } else {
                offline::import_metadata(&mut db, &bytes, source, at)?
            };
            println!("Processed {count} {command} records");
        }
        "snapshot" => {
            let months = match (text("--month"), text("--start"), text("--end")) {
                (Some(month), None, None) => vec![month.to_owned()],
                (None, Some(start), Some(end)) => offline::months(start, end)?,
                _ => return Err("provide --month or both --start and --end (exclusive)".into()),
            };
            let max_age = text("--max-age-days").unwrap_or("17").parse()?;
            if !database.is_file() {
                return Err(format!("catalog {} does not exist", database.display()).into());
            }
            let mut db = schema::open(&database)?;
            materialize_months(&mut db, months, max_age)?;
        }
        "stats" | "validate" => {
            let db = offline::open_read_only(&database)?;
            if command == "validate" {
                println!("Validated {} orbital records", offline::validate(&db)?);
            }
            let stats = offline::stats(&db)?.into_iter().collect::<BTreeMap<_, _>>();
            let expected = match (text("--start"), text("--end")) {
                (Some(start), Some(end)) => offline::months(start, end)?,
                (None, None) => match (stats.first_key_value(), stats.last_key_value()) {
                    (Some((first, _)), Some((last, _))) if first != last => {
                        let mut expected = offline::months(first, last)?;
                        expected.push(last.clone());
                        expected
                    }
                    _ => stats.keys().cloned().collect(),
                },
                _ => return Err("supply both --start and --end for a coverage range".into()),
            };
            for month in expected {
                match stats.get(&month) {
                    Some(count) if *count > 0 => println!("{month}: {count} members"),
                    _ => println!("{month}: missing coverage"),
                }
            }
        }
        "export" => {
            let db = offline::open_read_only(&database)?;
            let output = file("--output")?;
            offline::export(&db, &output)?;
            println!("Exported portable SQLite catalog to {}", output.display());
        }
        _ => unreachable!("validated operation"),
    }
    Ok(())
}

fn materialize_months(
    db: &mut rusqlite::Connection,
    months: Vec<String>,
    max_age: u32,
) -> Result<(), Box<dyn Error>> {
    for month in months {
        let count = hoth_catalog::offline::materialize(db, &month, max_age)?;
        if count == 0 {
            println!("{month}: missing coverage");
        } else {
            println!("{month}: {count} members");
        }
    }
    Ok(())
}

fn create_database_parent(path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}
