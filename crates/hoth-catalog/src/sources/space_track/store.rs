use super::{HistoryPlan, ImportError, PageQuery, SOURCE};
use crate::parse::ParsedElement;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{collections::BTreeSet, time::SystemTime};

pub(super) struct Page {
    pub(super) last_gp_id: i64,
    pub(super) complete: bool,
    pub(super) elements: Vec<(i64, ParsedElement)>,
    pub(super) checksum: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Checkpoint {
    pub(super) last_gp_id: i64,
    pub(super) complete: bool,
    seen_ids: BTreeSet<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MonthCoverage {
    pub month: String,
    pub complete: bool,
    /// Requested NORAD IDs without a stored record in this window so far.
    pub missing_ids: Vec<u32>,
}

/// Reads committed coverage, including months interrupted before completion.
pub fn coverage(
    connection: &Connection,
    plan: &HistoryPlan,
) -> Result<Vec<MonthCoverage>, ImportError> {
    plan.windows()
        .map(|window| {
            let month = window.key();
            let saved = checkpoint(connection, &plan.scope, &month)?;
            Ok(MonthCoverage {
                month,
                complete: saved.complete,
                missing_ids: plan
                    .ids()
                    .iter()
                    .copied()
                    .filter(|id| !saved.seen_ids.contains(id))
                    .collect(),
            })
        })
        .collect()
}

pub(super) fn checkpoint(
    connection: &Connection,
    scope: &str,
    month: &str,
) -> Result<Checkpoint, ImportError> {
    let saved: Option<(i64, bool, String)> = connection
        .query_row(
            "SELECT last_gp_id, complete, seen_ids FROM space_track_checkpoints
         WHERE object_scope = ?1 AND epoch_month = ?2",
            params![scope, month],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    match saved {
        Some((last_gp_id, complete, seen_ids)) => Ok(Checkpoint {
            last_gp_id,
            complete,
            seen_ids: serde_json::from_str(&seen_ids).map_err(|_| ImportError::CheckpointData)?,
        }),
        None => Ok(Checkpoint::default()),
    }
}

pub(super) fn commit_page(
    connection: &mut Connection,
    query: &PageQuery<'_>,
    month: &str,
    saved: &mut Checkpoint,
    page: Page,
) -> Result<usize, ImportError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if checkpoint(&transaction, query.object_scope, month)? != *saved {
        return Err(ImportError::CheckpointChanged);
    }

    let mut next = saved.clone();
    let mut inserted = 0;
    if !page.elements.is_empty() {
        let acquisition_id = format!(
            "{SOURCE}:gp_history:{}:{month}:{}",
            query.object_scope, query.after_gp_id
        );
        let acquired_at =
            DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Nanos, true);
        let source_ref = format!(
            "gp_history/{}/{month}/after/{}/sha256/{}",
            query.object_scope, query.after_gp_id, page.checksum
        );
        transaction.execute(
            "INSERT INTO catalog_acquisitions (id, source, acquired_at_utc, source_ref)
             VALUES (?1, ?2, ?3, ?4)",
            params![acquisition_id, SOURCE, acquired_at, source_ref],
        )?;
        for (id, parsed) in page.elements {
            inserted += insert_element(&transaction, &acquisition_id, id, &parsed)?;
            next.seen_ids.insert(parsed.element.norad_id.get());
        }
    }

    next.last_gp_id = page.last_gp_id;
    next.complete = page.complete;
    let seen = serde_json::to_string(&next.seen_ids).map_err(|_| ImportError::CheckpointData)?;
    transaction.execute(
        "INSERT INTO space_track_checkpoints
            (object_scope, epoch_month, last_gp_id, complete, seen_ids)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (object_scope, epoch_month) DO UPDATE SET
            last_gp_id = excluded.last_gp_id,
            complete = excluded.complete,
            seen_ids = excluded.seen_ids",
        params![
            query.object_scope,
            month,
            next.last_gp_id,
            next.complete,
            seen
        ],
    )?;
    transaction.commit()?;
    *saved = next;
    Ok(inserted)
}

fn insert_element(
    connection: &Connection,
    acquisition_id: &str,
    gp_id: i64,
    parsed: &ParsedElement,
) -> Result<usize, ImportError> {
    let element = &parsed.element;
    let norad_id = element.norad_id.get();
    let raw = &element.raw_source;
    let record_id = &raw.provenance.record_id;
    let existing: Option<String> = connection
        .query_row(
            "SELECT checksum FROM orbital_elements WHERE id = ?1",
            [record_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(checksum) = existing {
        return if checksum == raw.checksum {
            Ok(0)
        } else {
            Err(ImportError::RecordConflict(gp_id))
        };
    }
    connection.execute(
        "INSERT INTO space_objects (norad_id) VALUES (?1) ON CONFLICT (norad_id) DO NOTHING",
        [norad_id],
    )?;
    let epoch = element
        .epoch_utc
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
    let inserted = connection.execute(
        "INSERT INTO orbital_elements
            (id, norad_id, epoch_utc, source_epoch, format, raw_record, checksum, acquisition_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (norad_id, format, checksum) DO NOTHING",
        params![
            record_id,
            norad_id,
            epoch,
            raw.epoch_text,
            raw.format.as_str(),
            raw.record,
            raw.checksum,
            acquisition_id
        ],
    )?;
    Ok(inserted)
}
