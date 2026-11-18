use super::{
    finite, invalid, parse_decimal, parse_norad_decimal, Fields, ParseError, ParseWarning,
    ParsedElement,
};
use chrono::{DateTime, Datelike, NaiveDateTime};
use hoth_domain::{
    orbit::{ElementFormat, MeanElements, RecordProvenance},
    state::{NoradId, UtcInstant},
};
use serde::Deserialize;

/// Parses one OMM JSON object.
pub fn parse_omm_json(
    record: &[u8],
    provenance: RecordProvenance,
) -> Result<ParsedElement, ParseError> {
    if record.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(invalid("OMM JSON", "expected one object"));
    }
    let fields: OmmFields = serde_json::from_slice(record).map_err(ParseError::OmmJson)?;
    fields.normalize(record, provenance)
}

/// Parses an OMM CSV header and exactly one data row.
pub fn parse_omm_csv(
    record: &[u8],
    provenance: RecordProvenance,
) -> Result<ParsedElement, ParseError> {
    let mut reader = csv::Reader::from_reader(record);
    let mut rows = reader.deserialize::<OmmFields>();
    let fields = rows
        .next()
        .ok_or(ParseError::OmmCsvRecordCount)?
        .map_err(ParseError::OmmCsv)?;
    if rows.next().is_some() {
        return Err(ParseError::OmmCsvRecordCount);
    }
    fields.normalize(record, provenance)
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct OmmFields {
    norad_cat_id: CatalogId,
    epoch: String,
    inclination: Number,
    ra_of_asc_node: Number,
    eccentricity: Number,
    arg_of_pericenter: Number,
    mean_anomaly: Number,
    mean_motion: Number,
    bstar: Option<Number>,
    mean_motion_dot: Option<Number>,
    mean_motion_ddot: Option<Number>,
    center_name: Option<String>,
    ref_frame: Option<String>,
    time_system: Option<String>,
    mean_element_theory: Option<String>,
}

impl OmmFields {
    fn normalize(
        self,
        record: &[u8],
        provenance: RecordProvenance,
    ) -> Result<ParsedElement, ParseError> {
        let mut warnings = Vec::new();
        for (field, value, expected, reason) in [
            ("CENTER_NAME", self.center_name, "EARTH", "expected EARTH"),
            ("REF_FRAME", self.ref_frame, "TEME", "expected TEME"),
            ("TIME_SYSTEM", self.time_system, "UTC", "expected UTC"),
            (
                "MEAN_ELEMENT_THEORY",
                self.mean_element_theory,
                "SGP4",
                "expected SGP4",
            ),
        ] {
            match value
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(actual) if !actual.eq_ignore_ascii_case(expected) => {
                    return Err(invalid(field, reason));
                }
                Some(_) => {}
                None => warnings.push(ParseWarning::AssumedOmmMetadata {
                    field,
                    value: expected,
                }),
            }
        }

        Fields {
            norad_id: self.norad_cat_id.parse()?,
            epoch_utc: parse_epoch(&self.epoch)?,
            epoch_text: self.epoch,
            mean_elements: MeanElements {
                inclination_deg: self.inclination.parse("INCLINATION")?,
                right_ascension_deg: self.ra_of_asc_node.parse("RA_OF_ASC_NODE")?,
                eccentricity: self.eccentricity.parse("ECCENTRICITY")?,
                argument_of_perigee_deg: self.arg_of_pericenter.parse("ARG_OF_PERICENTER")?,
                mean_anomaly_deg: self.mean_anomaly.parse("MEAN_ANOMALY")?,
                mean_motion_rev_day: self.mean_motion.parse("MEAN_MOTION")?,
                bstar: Number::optional(self.bstar, "BSTAR")?,
                mean_motion_dot: Number::optional(self.mean_motion_dot, "MEAN_MOTION_DOT")?,
                mean_motion_ddot: Number::optional(self.mean_motion_ddot, "MEAN_MOTION_DDOT")?,
            },
        }
        .finish(ElementFormat::Omm, record, provenance, warnings)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CatalogId {
    Integer(u32),
    Text(String),
}

impl CatalogId {
    fn parse(self) -> Result<NoradId, ParseError> {
        match self {
            Self::Integer(id) => NoradId::new(id)
                .ok_or_else(|| invalid("NORAD_CAT_ID", "expected a positive 32-bit catalog ID")),
            Self::Text(value) => parse_norad_decimal(&value),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Number {
    Value(f64),
    Text(String),
}

impl Number {
    fn parse(self, field: &'static str) -> Result<f64, ParseError> {
        match self {
            Self::Value(value) => finite(value, field),
            Self::Text(value) => parse_decimal(&value, field),
        }
    }

    fn optional(value: Option<Self>, field: &'static str) -> Result<Option<f64>, ParseError> {
        match value {
            None => Ok(None),
            Some(Self::Text(text)) if text.trim().is_empty() => Ok(None),
            Some(value) => value.parse(field).map(Some),
        }
    }
}

fn parse_epoch(value: &str) -> Result<UtcInstant, ParseError> {
    let value = value.trim();
    if value
        .split_once('.')
        .is_some_and(|(_, fraction)| fraction.bytes().take_while(u8::is_ascii_digit).count() > 9)
    {
        return Err(invalid(
            "EPOCH",
            "at most nine fractional second digits are supported",
        ));
    }
    let instant = DateTime::parse_from_rfc3339(value)
        .map(|date| date.to_utc())
        .or_else(|_| {
            NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f").map(|date| date.and_utc())
        })
        .map_err(|_| {
            invalid(
                "EPOCH",
                "expected a calendar timestamp with optional UTC offset",
            )
        })?;
    if !(0..=9999).contains(&instant.year()) {
        return Err(invalid("EPOCH", "expected a four-digit UTC year"));
    }
    Ok(instant)
}
