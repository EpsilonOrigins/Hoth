//! Offline import, monthly materialization and portable catalog verification.

use crate::parse::{parse_omm_csv, parse_omm_json, parse_tle, sha256_hex, ParsedElement};
use crate::schema::SCHEMA_VERSION;
use chrono::{Months, NaiveDate, SecondsFormat, TimeDelta};
use hoth_domain::orbit::{OrbitalElement, RecordProvenance};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{error::Error, path::Path};

pub type CatalogResult<T> = Result<T, Box<dyn Error>>;

pub fn parse_record(
    format: &str,
    raw: &[u8],
    provenance: RecordProvenance,
) -> CatalogResult<ParsedElement> {
    match format {
        "tle" => Ok(parse_tle(raw, provenance)?),
        "omm" if raw.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{') => {
            Ok(parse_omm_json(raw, provenance)?)
        }
        "omm" => Ok(parse_omm_csv(raw, provenance)?),
        _ => Err(format!("unknown orbital format {format}").into()),
    }
}

fn acquisition(db: &Connection, source: &str, at: &str, bytes: &[u8]) -> CatalogResult<String> {
    if source.trim().is_empty() {
        return Err("source name must be nonblank".into());
    }
    let digest = sha256_hex(bytes);
    let id = format!("{source}:{at}:{digest}");
    db.execute(
        "INSERT INTO catalog_acquisitions VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO NOTHING",
        params![id, source, at, format!("sha256/{digest}")],
    )?;
    Ok(id)
}

pub fn import_elements(
    db: &mut Connection,
    format: &str,
    bytes: &[u8],
    source: &str,
    acquired_at: &str,
) -> CatalogResult<usize> {
    let at = normalized_utc(acquired_at)?;
    let records = match format {
        "tle" => {
            let lines: Vec<_> = bytes.split_inclusive(|byte| *byte == b'\n').collect();
            let mut records = Vec::new();
            let mut start = 0;
            for (index, line) in lines.iter().enumerate() {
                if line.starts_with(b"2 ") {
                    records.push(lines[start..=index].concat());
                    start = index + 1;
                }
            }
            if start != lines.len() {
                return Err("incomplete TLE at end of input".into());
            }
            records
        }
        "omm-json" if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[') => {
            serde_json::from_slice::<Vec<&RawValue>>(bytes)?
                .into_iter()
                .map(|row| row.get().as_bytes().to_vec())
                .collect()
        }
        "omm-json" => vec![bytes.to_vec()],
        "omm-csv" => {
            let mut reader = csv::Reader::from_reader(bytes);
            let header = reader
                .byte_headers()?
                .position()
                .map_or(0, |pos| pos.byte() as usize);
            let end_header = reader.position().byte() as usize;
            let mut records = Vec::new();
            let mut record = csv::ByteRecord::new();
            let mut start = end_header;
            while reader.read_byte_record(&mut record)? {
                let end = reader.position().byte() as usize;
                records.push([&bytes[header..end_header], &bytes[start..end]].concat());
                start = end;
            }
            records
        }
        _ => return Err("format must be tle, omm-json or omm-csv".into()),
    };
    if records.is_empty() {
        return Err("input contains no orbital records".into());
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let acquisition_id = acquisition(&tx, source, &at, bytes)?;
    let mut inserted = 0;
    let normalized_format = if format == "tle" { "tle" } else { "omm" };
    for (index, raw) in records.iter().enumerate() {
        let id = format!("offline:{}", sha256_hex(raw));
        let parsed = parse_record(
            normalized_format,
            raw,
            RecordProvenance {
                source: source.into(),
                record_id: id.clone(),
            },
        )
        .map_err(|error| format!("record {}: {error}", index + 1))?;
        let element = parsed.element;
        let norad = element.norad_id.get();
        let epoch = element
            .epoch_utc
            .to_rfc3339_opts(SecondsFormat::Nanos, true);
        tx.execute(
            "INSERT INTO space_objects(norad_id) VALUES (?1) ON CONFLICT DO NOTHING",
            [norad],
        )?;
        inserted += tx.execute(
            "INSERT INTO orbital_elements VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(norad_id, format, checksum) DO NOTHING",
            params![
                id,
                norad,
                epoch,
                element.raw_source.epoch_text,
                normalized_format,
                raw,
                element.raw_source.checksum,
                acquisition_id
            ],
        )?;
    }
    tx.commit()?;
    Ok(inserted)
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(alias = "NORAD_CAT_ID", deserialize_with = "metadata_id")]
    norad_id: u32,
    #[serde(alias = "OBJECT_NAME")]
    name: Option<String>,
    #[serde(alias = "OBJECT_TYPE")]
    object_type: Option<String>,
    #[serde(alias = "OPS_STATUS_CODE", alias = "OPERATIONAL_STATUS")]
    catalog_status: Option<String>,
    #[serde(alias = "RCS", deserialize_with = "metadata_rcs", default)]
    rcs_m2: Option<f64>,
}

fn metadata_id<'de, D: serde::Deserializer<'de>>(de: D) -> Result<u32, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Number(u32),
        Text(String),
    }
    match Id::deserialize(de)? {
        Id::Number(id) => Ok(id),
        Id::Text(text) => text.parse().map_err(serde::de::Error::custom),
    }
}

fn metadata_rcs<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Option<f64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Rcs {
        Number(f64),
        Text(String),
    }
    match Option::<Rcs>::deserialize(de)? {
        None => Ok(None),
        Some(Rcs::Number(value)) => Ok(Some(value)),
        Some(Rcs::Text(text))
            if matches!(
                text.trim().to_ascii_uppercase().as_str(),
                "" | "UNKNOWN" | "N/A" | "NULL"
            ) =>
        {
            Ok(None)
        }
        Some(Rcs::Text(text)) => text.parse().map(Some).map_err(serde::de::Error::custom),
    }
}

/// Newer acquisitions win independently for SATCAT fields and numeric RCS.
pub fn import_metadata(
    db: &mut Connection,
    bytes: &[u8],
    source: &str,
    acquired_at: &str,
) -> CatalogResult<usize> {
    let at = normalized_utc(acquired_at)?;
    let rows: Vec<Metadata> = serde_json::from_slice(bytes)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let id = acquisition(&tx, source, &at, bytes)?;
    let mut seen = std::collections::BTreeSet::new();
    for row in rows {
        if !seen.insert(row.norad_id) {
            return Err(format!("duplicate SATCAT NORAD ID {}", row.norad_id).into());
        }
        tx.execute(
            "INSERT INTO space_objects(norad_id) VALUES (?1) ON CONFLICT DO NOTHING",
            [row.norad_id],
        )?;
        let wins = |column: &str| -> rusqlite::Result<bool> {
            tx.query_row(&format!(
                "SELECT NOT EXISTS (SELECT 1 FROM space_objects s JOIN catalog_acquisitions a
                 ON a.id = s.{column} WHERE s.norad_id = ?1 AND (a.acquired_at_utc, a.id) >= (?2, ?3))"
            ), params![row.norad_id, at, id], |r| r.get(0))
        };
        if (row.name.is_some() || row.object_type.is_some() || row.catalog_status.is_some())
            && wins("metadata_acquisition_id")?
        {
            tx.execute(
                "UPDATE space_objects SET name=?2, object_type=?3, catalog_status=?4,
                 metadata_acquisition_id=?5 WHERE norad_id=?1",
                params![
                    row.norad_id,
                    row.name,
                    row.object_type,
                    row.catalog_status,
                    id
                ],
            )?;
        }
        if let Some(rcs) = row.rcs_m2 {
            if !rcs.is_finite() || rcs < 0.0 {
                return Err(format!("invalid RCS for NORAD {}", row.norad_id).into());
            }
            if wins("rcs_acquisition_id")? {
                tx.execute(
                    "UPDATE space_objects SET rcs_m2=?2, rcs_acquisition_id=?3 WHERE norad_id=?1",
                    params![row.norad_id, rcs, id],
                )?;
            }
        }
    }
    tx.commit()?;
    Ok(seen.len())
}

fn normalized_utc(value: &str) -> CatalogResult<String> {
    Ok(chrono::DateTime::parse_from_rfc3339(value)?
        .to_utc()
        .to_rfc3339_opts(SecondsFormat::Nanos, true))
}

fn month(value: &str) -> CatalogResult<NaiveDate> {
    let date = NaiveDate::parse_from_str(&format!("{value}-01"), "%Y-%m-%d")?;
    if date.format("%Y-%m").to_string() != value || value.len() != 7 {
        return Err("expected YYYY-MM".into());
    }
    Ok(date)
}

pub fn months(start: &str, end: &str) -> CatalogResult<Vec<String>> {
    let (start, end) = (month(start)?, month(end)?);
    if start >= end {
        return Err("start must precede the exclusive end month".into());
    }
    Ok(
        std::iter::successors(Some(start), |date| date.checked_add_months(Months::new(1)))
            .take_while(|date| *date < end)
            .map(|date| date.format("%Y-%m").to_string())
            .collect(),
    )
}

/// Replaces membership atomically. Empty months remain visible in coverage reports.
pub fn materialize(
    db: &mut Connection,
    target_month: &str,
    max_age_days: u32,
) -> CatalogResult<usize> {
    if !(1..=366).contains(&max_age_days) {
        return Err("max age must be 1 through 366 days".into());
    }
    let anchor = (month(target_month)? + TimeDelta::days(14))
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_utc();
    let delta = TimeDelta::days(i64::from(max_age_days));
    let timestamp = |instant: chrono::DateTime<chrono::Utc>| {
        instant.to_rfc3339_opts(SecondsFormat::Nanos, true)
    };
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO catalog_snapshots VALUES (?1, ?1) ON CONFLICT(target_month) DO NOTHING",
        [target_month],
    )?;
    let id: String = tx.query_row(
        "SELECT id FROM catalog_snapshots WHERE target_month=?1",
        [target_month],
        |r| r.get(0),
    )?;
    tx.execute("DELETE FROM snapshot_members WHERE snapshot_id=?1", [&id])?;
    let count = tx.execute(
        "INSERT INTO snapshot_members
         SELECT ?1, norad_id, id FROM (
           SELECT norad_id, id, ROW_NUMBER() OVER (
             PARTITION BY norad_id ORDER BY
             abs((unixepoch(substr(epoch_utc, 1, 19)) - ?4) * 1000000000
                 + CAST(substr(epoch_utc, 21, 9) AS INTEGER)), id
           ) AS rank
           FROM orbital_elements WHERE epoch_utc BETWEEN ?2 AND ?3
         ) WHERE rank=1",
        params![
            id,
            timestamp(anchor - delta),
            timestamp(anchor + delta),
            anchor.timestamp()
        ],
    )?;
    tx.commit()?;
    Ok(count)
}

type StoredRecord = (String, String, u32, String, String, String, Vec<u8>, String);

fn read_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRecord> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn verify_record(record: StoredRecord) -> CatalogResult<OrbitalElement> {
    let (id, source, norad, epoch, original_epoch, format, raw, checksum) = record;
    let element = parse_record(
        &format,
        &raw,
        RecordProvenance {
            source,
            record_id: id.clone(),
        },
    )?
    .element;
    if element.norad_id.get() != norad
        || element
            .epoch_utc
            .to_rfc3339_opts(SecondsFormat::Nanos, true)
            != epoch
        || element.raw_source.epoch_text != original_epoch
        || element.raw_source.checksum != checksum
    {
        return Err(format!("stored orbital element {id} failed verification").into());
    }
    Ok(element)
}

const RECORD_FIELDS: &str = "e.id, a.source, e.norad_id, e.epoch_utc, e.source_epoch,
                             e.format, e.raw_record, e.checksum";

pub fn open_read_only(path: &Path) -> CatalogResult<Connection> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        return Err(format!("catalog schema {version}; expected {SCHEMA_VERSION}").into());
    }
    Ok(db)
}

pub fn snapshot(db: &Connection, month: &str) -> CatalogResult<(String, Vec<OrbitalElement>)> {
    let id: String = db
        .query_row(
            "SELECT id FROM catalog_snapshots WHERE target_month=?1",
            [month],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| format!("snapshot {month} does not exist"))?;
    let mut query = db.prepare(&format!(
        "SELECT {RECORD_FIELDS} FROM snapshot_members m
         LEFT JOIN orbital_elements e ON m.element_id=e.id AND m.norad_id=e.norad_id
         LEFT JOIN catalog_acquisitions a ON e.acquisition_id=a.id
         WHERE m.snapshot_id=?1 ORDER BY m.norad_id"
    ))?;
    let mut elements = Vec::new();
    for row in query.query_map([&id], read_record)? {
        elements.push(verify_record(row?)?);
    }
    if elements.is_empty() {
        return Err(format!("snapshot {month} has no eligible members").into());
    }
    Ok((id, elements))
}

pub fn stats(db: &Connection) -> CatalogResult<Vec<(String, i64)>> {
    let mut query = db.prepare(
        "SELECT s.target_month, count(m.norad_id) FROM catalog_snapshots s
         LEFT JOIN snapshot_members m ON s.id=m.snapshot_id
         GROUP BY s.target_month ORDER BY s.target_month",
    )?;
    let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn validate(db: &Connection) -> CatalogResult<usize> {
    let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        return Err(format!("catalog schema {version}; expected {SCHEMA_VERSION}").into());
    }
    let integrity: String = db.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" || db.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Err(
            format!("catalog SQLite integrity or foreign key check failed: {integrity}").into(),
        );
    }
    let mut query = db.prepare(&format!(
        "SELECT {RECORD_FIELDS} FROM orbital_elements e JOIN catalog_acquisitions a ON e.acquisition_id=a.id"
    ))?;
    let mut count = 0;
    for row in query.query_map([], read_record)? {
        verify_record(row?)?;
        count += 1;
    }
    Ok(count)
}

/// Exports a consistent SQLite snapshot and verifies all records in the copy.
pub fn export(db: &Connection, output: &Path) -> CatalogResult<()> {
    let temporary = tempfile::NamedTempFile::new_in(
        output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    db.execute(
        "VACUUM INTO ?1",
        [temporary
            .path()
            .to_str()
            .ok_or("output path must be UTF-8")?],
    )?;
    let exported = open_read_only(temporary.path())?;
    validate(&exported)?;
    drop(exported);
    temporary.persist_noclobber(output)?;
    Ok(())
}
