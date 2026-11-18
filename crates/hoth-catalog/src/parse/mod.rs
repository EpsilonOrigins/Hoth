//! Parse one local TLE or OMM record into the shared orbital-element contract.

mod omm;
mod tle;

pub use omm::{parse_omm_csv, parse_omm_json};
pub use tle::parse_tle;

use hoth_domain::{
    orbit::{ElementFormat, MeanElements, OrbitalElement, RawOrbitalSource, RecordProvenance},
    state::{NoradId, UtcInstant},
};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fmt::{self, Write as _},
    str::Utf8Error,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedElement {
    pub element: OrbitalElement,
    pub warnings: Vec<ParseWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseWarning {
    AssumedOmmMetadata {
        field: &'static str,
        value: &'static str,
    },
    MissingDragField(&'static str),
}

#[derive(Debug)]
pub enum ParseError {
    InvalidProvenance,
    TleEncoding(Utf8Error),
    TleRecordCount,
    TleLine {
        line: u8,
        reason: &'static str,
    },
    TleChecksum {
        line: u8,
        expected: u8,
        actual: u8,
    },
    TleNoradMismatch {
        line1: NoradId,
        line2: NoradId,
    },
    OmmJson(serde_json::Error),
    OmmCsv(csv::Error),
    OmmCsvRecordCount,
    InvalidField {
        field: &'static str,
        reason: &'static str,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProvenance => formatter.write_str("source and record ID must be nonblank"),
            Self::TleEncoding(source) => write!(formatter, "TLE is not valid UTF-8: {source}"),
            Self::TleRecordCount => {
                formatter.write_str("expected two TLE lines and an optional title")
            }
            Self::TleLine { line, reason } => write!(formatter, "TLE line {line}: {reason}"),
            Self::TleChecksum {
                line,
                expected,
                actual,
            } => write!(
                formatter,
                "TLE line {line} checksum is {actual}, expected {expected}"
            ),
            Self::TleNoradMismatch { line1, line2 } => write!(
                formatter,
                "TLE NORAD IDs differ between lines ({} and {})",
                line1.get(),
                line2.get()
            ),
            Self::OmmJson(source) => write!(formatter, "invalid OMM JSON: {source}"),
            Self::OmmCsv(source) => write!(formatter, "invalid OMM CSV: {source}"),
            Self::OmmCsvRecordCount => {
                formatter.write_str("expected an OMM CSV header and one data row")
            }
            Self::InvalidField { field, reason } => write!(formatter, "{field}: {reason}"),
        }
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::TleEncoding(source) => Some(source),
            Self::OmmJson(source) => Some(source),
            Self::OmmCsv(source) => Some(source),
            _ => None,
        }
    }
}

struct Fields {
    norad_id: NoradId,
    epoch_utc: UtcInstant,
    epoch_text: String,
    mean_elements: MeanElements,
}

impl Fields {
    fn finish(
        self,
        format: ElementFormat,
        record: &[u8],
        provenance: RecordProvenance,
        mut warnings: Vec<ParseWarning>,
    ) -> Result<ParsedElement, ParseError> {
        if provenance.source.trim().is_empty() || provenance.record_id.trim().is_empty() {
            return Err(ParseError::InvalidProvenance);
        }
        let mut mean_elements = self.mean_elements;
        if !(0.0..=180.0).contains(&mean_elements.inclination_deg) {
            return Err(invalid("INCLINATION", "expected 0 through 180 degrees"));
        }
        if !(0.0..1.0).contains(&mean_elements.eccentricity) {
            return Err(invalid("ECCENTRICITY", "expected a value in [0, 1)"));
        }
        if mean_elements.mean_motion_rev_day <= 0.0 {
            return Err(invalid("MEAN_MOTION", "expected a positive rate"));
        }
        for (field, angle) in [
            ("RA_OF_ASC_NODE", &mut mean_elements.right_ascension_deg),
            (
                "ARG_OF_PERICENTER",
                &mut mean_elements.argument_of_perigee_deg,
            ),
            ("MEAN_ANOMALY", &mut mean_elements.mean_anomaly_deg),
        ] {
            if !(0.0..=360.0).contains(angle) {
                return Err(invalid(field, "expected 0 through 360 degrees"));
            }
            if *angle == 360.0 {
                *angle = 0.0;
            }
        }
        for (field, value) in [
            ("BSTAR", mean_elements.bstar),
            ("MEAN_MOTION_DOT", mean_elements.mean_motion_dot),
            ("MEAN_MOTION_DDOT", mean_elements.mean_motion_ddot),
        ] {
            if value.is_none() {
                warnings.push(ParseWarning::MissingDragField(field));
            }
        }
        Ok(ParsedElement {
            element: OrbitalElement {
                norad_id: self.norad_id,
                epoch_utc: self.epoch_utc,
                mean_elements,
                raw_source: RawOrbitalSource {
                    format,
                    epoch_text: self.epoch_text,
                    record: record.to_vec(),
                    checksum: sha256_hex(record),
                    provenance,
                },
            },
            warnings,
        })
    }
}

fn invalid(field: &'static str, reason: &'static str) -> ParseError {
    ParseError::InvalidField { field, reason }
}

fn parse_norad_decimal(value: &str) -> Result<NoradId, ParseError> {
    let value = value.trim();
    if value.bytes().all(|character| character.is_ascii_digit()) {
        if let Some(norad_id) = value.parse::<u32>().ok().and_then(NoradId::new) {
            return Ok(norad_id);
        }
    }
    Err(invalid(
        "NORAD_CAT_ID",
        "expected a positive 32-bit catalog ID",
    ))
}

fn parse_decimal(value: &str, field: &'static str) -> Result<f64, ParseError> {
    let number = value
        .trim()
        .parse()
        .map_err(|_| invalid(field, "expected a number"))?;
    finite(number, field)
}

fn finite(value: f64, field: &'static str) -> Result<f64, ParseError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(field, "expected a finite value"))
    }
}

fn sha256_hex(record: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(record) {
        write!(hex, "{byte:02x}").expect("writing to a String is infallible");
    }
    hex
}
