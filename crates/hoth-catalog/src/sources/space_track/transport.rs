use super::{ImportError, PageQuery};
use std::{
    env,
    io::ErrorKind,
    time::{Duration, SystemTime},
};
use ureq::{http::Response, Agent, Body};

const LOGIN_URL: &str = "https://www.space-track.org/ajaxauth/login";
const LOGIN_BODY_LIMIT: u64 = 8 * 1024;
pub(super) const PAGE_BODY_LIMIT: u64 = 8 * 1024 * 1024;

/// Credentials are deliberately neither printable nor serializable.
pub struct Credentials {
    identity: String,
    password: String,
}

impl Credentials {
    pub fn from_env() -> Result<Self, ImportError> {
        fn read(name: &'static str) -> Result<String, ImportError> {
            env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or(ImportError::Credentials(name))
        }

        Self::new(read("SPACE_TRACK_IDENTITY")?, read("SPACE_TRACK_PASSWORD")?)
    }

    pub fn new(identity: String, password: String) -> Result<Self, ImportError> {
        if identity.trim().is_empty() || password.trim().is_empty() {
            return Err(ImportError::Plan(
                "Space-Track credentials must be nonblank",
            ));
        }
        Ok(Self { identity, password })
    }
}

pub struct Reply {
    pub status: u16,
    pub retry_after: Option<Duration>,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    Timeout,
    ResponseTooLarge,
    Connection,
}

/// Injected HTTP boundary. Errors carry no URLs, bodies, cookies, or credentials.
pub trait HistoryTransport {
    fn login(&mut self, credentials: &Credentials) -> Result<Reply, TransportError>;
    fn fetch(&mut self, query: &PageQuery<'_>) -> Result<Reply, TransportError>;
}

pub struct SpaceTrackHttp {
    agent: Agent,
}

impl SpaceTrackHttp {
    pub fn new() -> Self {
        let agent = Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self { agent }
    }
}

impl Default for SpaceTrackHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryTransport for SpaceTrackHttp {
    fn login(&mut self, credentials: &Credentials) -> Result<Reply, TransportError> {
        let response = self
            .agent
            .post(LOGIN_URL)
            .send_form([
                ("identity", credentials.identity.as_str()),
                ("password", credentials.password.as_str()),
            ])
            .map_err(classify)?;
        reply(response, LOGIN_BODY_LIMIT)
    }

    fn fetch(&mut self, query: &PageQuery<'_>) -> Result<Reply, TransportError> {
        let mut response = self.agent.get(query.url()).call().map_err(classify)?;
        if response.status().is_redirection()
            && response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    value.starts_with("/auth/login")
                        || value.starts_with("https://www.space-track.org/auth/login")
                })
        {
            *response.status_mut() = ureq::http::StatusCode::UNAUTHORIZED;
        }
        reply(response, PAGE_BODY_LIMIT)
    }
}

fn reply(mut response: Response<Body>, limit: u64) -> Result<Reply, TransportError> {
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| retry_after(value, SystemTime::now()));
    let body = if status == 200 {
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(classify)?
    } else {
        Vec::new()
    };
    Ok(Reply {
        status,
        retry_after,
        body,
    })
}

fn classify(error: ureq::Error) -> TransportError {
    match error {
        ureq::Error::Timeout(_) => TransportError::Timeout,
        ureq::Error::Io(error) if error.kind() == ErrorKind::TimedOut => TransportError::Timeout,
        ureq::Error::BodyExceedsLimit(_) => TransportError::ResponseTooLarge,
        _ => TransportError::Connection,
    }
}

fn retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let deadline = chrono::DateTime::parse_from_rfc2822(value).ok()?.to_utc();
    Some(
        deadline
            .signed_duration_since(chrono::DateTime::<chrono::Utc>::from(now))
            .to_std()
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_retry_after_seconds_and_http_dates() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_445_412_450);
        assert_eq!(retry_after("30", now), Some(Duration::from_secs(30)));
        assert_eq!(
            retry_after("Wed, 21 Oct 2015 07:28:00 GMT", now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(retry_after("invalid", now), None);
    }
}
