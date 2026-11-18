use hoth_catalog::{
    schema,
    sources::space_track::{
        coverage, Credentials, HistoryImporter, HistoryPlan, SpaceTrackHttp, DEFAULT_PAGE_SIZE,
    },
};
use std::{error::Error, ffi::OsString, fs, path::PathBuf, process::ExitCode, thread};

const USAGE: &str =
    "Usage: hoth catalog bootstrap --start YYYY-MM --end YYYY-MM [--norad ID,ID] [--database PATH] [--dry-run]";

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
        if let Some(parent) = self
            .database
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
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
        println!("Imported {count} new Space-Track orbital records.");
        Ok(())
    }
}
