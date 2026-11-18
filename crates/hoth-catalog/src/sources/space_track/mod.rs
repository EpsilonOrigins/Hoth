//! Authenticated GP History acquisition with restartable, atomic page commits.

mod plan;
mod store;
mod transport;

pub use plan::{HistoryPlan, MonthWindow, PageQuery};
pub use store::{coverage, MonthCoverage};
pub use transport::{Credentials, HistoryTransport, Reply, SpaceTrackHttp, TransportError};

use crate::parse::{parse_omm_json, sha256_hex, ParsedElement};
use hoth_domain::orbit::RecordProvenance;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{value::RawValue, Value};
use std::{error::Error, fmt, time::Duration};
use store::{checkpoint, commit_page, Page};

pub const DEFAULT_PAGE_SIZE: usize = 1_000;
const REQUEST_INTERVAL: Duration = Duration::from_secs(13);
const MAX_RETRIES: u32 = 3;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(120);
const SOURCE: &str = "space-track";

#[derive(Debug)]
pub enum ImportError {
    Plan(&'static str),
    Credentials(&'static str),
    Authentication,
    Http(u16),
    Transport(TransportError),
    RetryAfter(Duration),
    InvalidPage(&'static str),
    InvalidElement(i64),
    RecordConflict(i64),
    CheckpointChanged,
    CheckpointData,
    Database(rusqlite::Error),
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(message) => formatter.write_str(message),
            Self::Credentials(name) => write!(formatter, "set {name} to use Space-Track"),
            Self::Authentication => formatter.write_str("Space-Track authentication failed"),
            Self::Http(status) => write!(formatter, "Space-Track returned HTTP {status}"),
            Self::Transport(TransportError::Timeout) => {
                formatter.write_str("Space-Track request timed out")
            }
            Self::Transport(TransportError::ResponseTooLarge) => {
                formatter.write_str("Space-Track response exceeded the configured limit")
            }
            Self::Transport(TransportError::Connection) => {
                formatter.write_str("Space-Track transport failed")
            }
            Self::RetryAfter(delay) => write!(
                formatter,
                "Space-Track requested a retry delay of {} seconds. Resume after this delay.",
                delay.as_secs()
            ),
            Self::InvalidPage(reason) => write!(formatter, "invalid Space-Track page: {reason}"),
            Self::InvalidElement(id) => write!(
                formatter,
                "Space-Track GP_ID {id} contains an invalid OMM record"
            ),
            Self::RecordConflict(id) => write!(
                formatter,
                "stored Space-Track GP_ID {id} has different orbital data"
            ),
            Self::CheckpointChanged => formatter.write_str(
                "import checkpoint changed during acquisition. Resume with one importer.",
            ),
            Self::CheckpointData => {
                formatter.write_str("catalog contains invalid Space-Track checkpoint data")
            }
            Self::Database(source) => write!(formatter, "catalog write failed: {source}"),
        }
    }
}

impl Error for ImportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(source) => Some(source),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for ImportError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Database(source)
    }
}

pub struct HistoryImporter<T, W> {
    transport: T,
    wait: W,
    credentials: Credentials,
    page_size: usize,
    authenticated: bool,
    next_delay: Duration,
}

impl<T: HistoryTransport, W: FnMut(Duration)> HistoryImporter<T, W> {
    pub fn new(
        transport: T,
        wait: W,
        credentials: Credentials,
        page_size: usize,
    ) -> Result<Self, ImportError> {
        if !(1..=DEFAULT_PAGE_SIZE).contains(&page_size) {
            return Err(ImportError::Plan("page size must be between 1 and 1,000"));
        }
        Ok(Self {
            transport,
            wait,
            credentials,
            page_size,
            authenticated: false,
            next_delay: Duration::ZERO,
        })
    }

    /// Imports unfinished windows and returns how many new element rows were inserted.
    pub fn run(
        &mut self,
        connection: &mut Connection,
        plan: &HistoryPlan,
    ) -> Result<usize, ImportError> {
        plan.validate_live()?;
        let mut inserted = 0;
        for window in plan.windows() {
            let month = window.key();
            let mut saved = checkpoint(connection, &plan.scope, &month)?;
            while !saved.complete {
                let query = PageQuery {
                    window,
                    object_scope: &plan.scope,
                    after_gp_id: saved.last_gp_id,
                    limit: self.page_size,
                };
                let body = self.fetch(&query)?;
                let page = parse_page(&body, &query, plan.ids())?;
                inserted += commit_page(connection, &query, &month, &mut saved, page)?;
            }
        }
        Ok(inserted)
    }

    fn login(&mut self) -> Result<(), ImportError> {
        let reply = self.request(T::login)?;
        let response: Value =
            serde_json::from_slice(&reply.body).map_err(|_| ImportError::Authentication)?;
        if reply.status != 200 || response.get("Login").and_then(Value::as_str) == Some("Failed") {
            return Err(ImportError::Authentication);
        }
        self.authenticated = true;
        Ok(())
    }

    fn fetch(&mut self, query: &PageQuery<'_>) -> Result<Vec<u8>, ImportError> {
        if !self.authenticated {
            self.login()?;
        }
        let mut reply = self.request(|transport, _| transport.fetch(query))?;
        if reply.status == 401 {
            self.authenticated = false;
            self.login()?;
            reply = self.request(|transport, _| transport.fetch(query))?;
        }
        match reply.status {
            200 => Ok(reply.body),
            401 => {
                self.authenticated = false;
                Err(ImportError::Authentication)
            }
            status => Err(ImportError::Http(status)),
        }
    }

    fn request(
        &mut self,
        mut send: impl FnMut(&mut T, &Credentials) -> Result<Reply, TransportError>,
    ) -> Result<Reply, ImportError> {
        let mut retries = 0;
        loop {
            if !self.next_delay.is_zero() {
                (self.wait)(self.next_delay);
            }
            self.next_delay = REQUEST_INTERVAL;
            let (error, retry_after) = match send(&mut self.transport, &self.credentials) {
                Ok(reply) if reply.status == 429 || (500..=599).contains(&reply.status) => {
                    (ImportError::Http(reply.status), reply.retry_after)
                }
                Ok(reply) => return Ok(reply),
                Err(TransportError::Timeout) => {
                    (ImportError::Transport(TransportError::Timeout), None)
                }
                Err(error) => return Err(ImportError::Transport(error)),
            };
            if let Some(delay) = retry_after {
                if delay > MAX_RETRY_DELAY {
                    return Err(ImportError::RetryAfter(delay));
                }
            }
            let backoff = Duration::from_secs(15 << retries);
            self.next_delay = backoff
                .max(retry_after.unwrap_or_default())
                .max(REQUEST_INTERVAL);
            if retries == MAX_RETRIES {
                return Err(error);
            }
            retries += 1;
        }
    }
}

#[derive(Deserialize)]
struct RowIdentity {
    #[serde(rename = "GP_ID")]
    gp_id: GpId,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum GpId {
    Number(i64),
    Text(String),
}

fn parse_page(body: &[u8], query: &PageQuery<'_>, requested: &[u32]) -> Result<Page, ImportError> {
    if body.len() as u64 > transport::PAGE_BODY_LIMIT {
        return Err(ImportError::Transport(TransportError::ResponseTooLarge));
    }
    let raw: Vec<&RawValue> = serde_json::from_slice(body)
        .map_err(|_| ImportError::InvalidPage("expected an OMM JSON array"))?;
    if raw.len() > query.limit {
        return Err(ImportError::InvalidPage(
            "response exceeded the requested row limit",
        ));
    }
    let mut last_gp_id = query.after_gp_id;
    let complete = raw.len() < query.limit;
    let mut elements: Vec<(i64, ParsedElement)> = Vec::with_capacity(raw.len());
    for row in raw {
        let identity: RowIdentity = serde_json::from_str(row.get())
            .map_err(|_| ImportError::InvalidPage("a row is missing a valid GP_ID"))?;
        let id = match identity.gp_id {
            GpId::Number(id) => id,
            GpId::Text(text) => text
                .parse()
                .map_err(|_| ImportError::InvalidPage("a row has an invalid GP_ID"))?,
        };
        if id <= last_gp_id {
            return Err(ImportError::InvalidPage(
                "GP_ID must strictly advance in ascending order",
            ));
        }

        let parsed = parse_omm_json(
            row.get().as_bytes(),
            RecordProvenance {
                source: SOURCE.to_owned(),
                record_id: format!("{SOURCE}:{id}"),
            },
        )
        .map_err(|_| ImportError::InvalidElement(id))?;
        if requested
            .binary_search(&parsed.element.norad_id.get())
            .is_err()
        {
            return Err(ImportError::InvalidPage(
                "row is outside the requested NORAD scope",
            ));
        }
        if !(query.window.start..query.window.end).contains(&parsed.element.epoch_utc.date_naive())
        {
            return Err(ImportError::InvalidPage(
                "row epoch is outside the requested UTC month",
            ));
        }
        elements.push((id, parsed));
        last_gp_id = id;
    }
    Ok(Page {
        last_gp_id,
        complete,
        elements,
        checksum: sha256_hex(body),
    })
}
