use super::{invalid, parse_decimal, parse_norad_decimal, Fields, ParseError, ParsedElement};
use chrono::{NaiveDate, TimeDelta};
use hoth_domain::{
    orbit::{ElementFormat, MeanElements, RecordProvenance},
    state::{NoradId, UtcInstant},
};

/// Parses two fixed-width element lines, optionally preceded by a title.
pub fn parse_tle(record: &[u8], provenance: RecordProvenance) -> Result<ParsedElement, ParseError> {
    let mut lines = str::from_utf8(record)
        .map_err(ParseError::TleEncoding)?
        .lines();
    let (line1, line2) = match (lines.next(), lines.next(), lines.next(), lines.next()) {
        (Some(line1), Some(line2), None, None) | (Some(_), Some(line1), Some(line2), None) => {
            (line1, line2)
        }
        _ => return Err(ParseError::TleRecordCount),
    };
    validate_line(line1, 1, &[1, 8, 17, 32, 43, 52, 61, 63])?;
    validate_line(line2, 2, &[1, 7, 16, 25, 33, 42, 51])?;

    let norad_id = parse_norad(&line1[2..7])?;
    let second_norad_id = parse_norad(&line2[2..7])?;
    if norad_id != second_norad_id {
        return Err(ParseError::TleNoradMismatch {
            line1: norad_id,
            line2: second_norad_id,
        });
    }

    let epoch_text = &line1[18..32];
    Fields {
        norad_id,
        epoch_utc: parse_epoch(epoch_text)?,
        epoch_text: epoch_text.to_owned(),
        mean_elements: MeanElements {
            inclination_deg: parse_decimal(&line2[8..16], "INCLINATION")?,
            right_ascension_deg: parse_decimal(&line2[17..25], "RA_OF_ASC_NODE")?,
            eccentricity: f64::from(digits(&line2[26..33], "ECCENTRICITY")?) / 10_000_000.0,
            argument_of_perigee_deg: parse_decimal(&line2[34..42], "ARG_OF_PERICENTER")?,
            mean_anomaly_deg: parse_decimal(&line2[43..51], "MEAN_ANOMALY")?,
            mean_motion_rev_day: parse_decimal(&line2[52..63], "MEAN_MOTION")?,
            mean_motion_dot: optional_decimal(&line1[33..43], "MEAN_MOTION_DOT")?,
            mean_motion_ddot: compact_decimal(&line1[44..52], "MEAN_MOTION_DDOT")?,
            bstar: compact_decimal(&line1[53..61], "BSTAR")?,
        },
    }
    .finish(ElementFormat::Tle, record, provenance, Vec::new())
}

fn validate_line(text: &str, line: u8, separators: &[usize]) -> Result<(), ParseError> {
    let content = text.as_bytes();
    if content.len() != 69 || !content.iter().all(|byte| (b' '..=b'~').contains(byte)) {
        return Err(ParseError::TleLine {
            line,
            reason: "expected 69 printable ASCII columns",
        });
    }
    if content[0] != b'0' + line || separators.iter().any(|&index| content[index] != b' ') {
        return Err(ParseError::TleLine {
            line,
            reason: "invalid line number or fixed-width separator",
        });
    }
    if !content[68].is_ascii_digit() {
        return Err(ParseError::TleLine {
            line,
            reason: "checksum column must be a digit",
        });
    }
    let expected = content[..68]
        .iter()
        .map(|byte| match byte {
            b'0'..=b'9' => u16::from(*byte - b'0'),
            b'-' => 1,
            _ => 0,
        })
        .sum::<u16>()
        % 10;
    let actual = content[68] - b'0';
    if actual != expected as u8 {
        return Err(ParseError::TleChecksum {
            line,
            expected: expected as u8,
            actual,
        });
    }
    Ok(())
}

fn parse_norad(value: &str) -> Result<NoradId, ParseError> {
    const ALPHA5: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    if let Some(prefix) = ALPHA5
        .iter()
        .position(|&letter| letter == value.as_bytes()[0])
    {
        let suffix = digits(&value[1..], "NORAD_CAT_ID")?;
        let id = (10 + prefix as u32) * 10_000 + suffix;
        return NoradId::new(id).ok_or_else(|| invalid("NORAD_CAT_ID", "expected a positive ID"));
    }
    parse_norad_decimal(value)
}

fn parse_epoch(value: &str) -> Result<UtcInstant, ParseError> {
    if value.as_bytes()[5] != b'.' {
        return Err(invalid("EPOCH", "expected TLE YYDDD.dddddddd"));
    }
    let year = digits(&value[..2], "EPOCH")? as i32;
    let year = if year < 57 { 2000 + year } else { 1900 + year };
    let ordinal = digits(&value[2..5], "EPOCH")?;
    let midnight = NaiveDate::from_yo_opt(year, ordinal)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .ok_or_else(|| invalid("EPOCH", "ordinal day is outside the source year"))?;
    // Each 1e-8 day increment is exactly 864,000 nanoseconds.
    let nanoseconds = i64::from(digits(&value[6..], "EPOCH")?) * 864_000;
    Ok((midnight + TimeDelta::nanoseconds(nanoseconds)).and_utc())
}

fn digits(value: &str, field: &'static str) -> Result<u32, ParseError> {
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid(field, "expected decimal digits in the TLE columns"));
    }
    value
        .parse()
        .map_err(|_| invalid(field, "expected a TLE integer"))
}

fn optional_decimal(value: &str, field: &'static str) -> Result<Option<f64>, ParseError> {
    if value.trim().is_empty() {
        Ok(None)
    } else {
        parse_decimal(value, field).map(Some)
    }
}

fn compact_decimal(value: &str, field: &'static str) -> Result<Option<f64>, ParseError> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let [sign @ (b' ' | b'+' | b'-'), _, _, _, _, _, exponent_sign @ (b'+' | b'-'), exponent @ b'0'..=b'9'] =
        value.as_bytes()
    else {
        return Err(invalid(
            field,
            "expected a TLE mantissa and signed exponent",
        ));
    };
    let mantissa = f64::from(digits(&value[1..6], field)?) / 100_000.0;
    let mantissa = if *sign == b'-' { -mantissa } else { mantissa };
    let exponent = i32::from(*exponent - b'0');
    let exponent = if *exponent_sign == b'-' {
        -exponent
    } else {
        exponent
    };
    Ok(Some(mantissa * 10.0_f64.powi(exponent)))
}
