//! Local mock execution-control protocol over TCP. The runtime owns time semantics.

use hoth_domain::state::SimulationUtc;
use hoth_runtime::{Command, Runner};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    time::Duration,
};

const COMMAND_LIMIT: u64 = 8192;
const IO_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u8,
    run_id: String,
    sequence: u64,
    command: WireCommand,
    at_utc: Option<SimulationUtc>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum WireCommand {
    Start,
    Pause,
    Resume,
    Seek,
    Tick,
    Stop,
}

impl Request {
    fn command(&self) -> Result<Command, &'static str> {
        match (self.command, self.at_utc) {
            (WireCommand::Seek, Some(at)) => Ok(Command::Seek(at)),
            (WireCommand::Tick, Some(at)) => Ok(Command::Tick(at)),
            (WireCommand::Seek | WireCommand::Tick, None) => Err("seek and tick require at_utc"),
            (_, Some(_)) => Err("at_utc is only accepted for seek and tick"),
            (WireCommand::Start, None) => Ok(Command::Start),
            (WireCommand::Pause, None) => Ok(Command::Pause),
            (WireCommand::Resume, None) => Ok(Command::Resume),
            (WireCommand::Stop, None) => Ok(Command::Stop),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct Reply {
    schema_version: u8,
    run_id: String,
    sequence: Option<u64>,
    accepted: bool,
    state: &'static str,
    at_utc: Option<SimulationUtc>,
    error: Option<String>,
}

pub struct Controller {
    listener: TcpListener,
    run_id: String,
    last: Option<(Request, Reply)>,
}

impl Controller {
    pub fn bind(address: SocketAddr, run_id: String) -> io::Result<Self> {
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            run_id,
            last: None,
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub fn poll(&mut self, runner: &mut Runner) -> io::Result<()> {
        let (mut stream, _) = match self.listener.accept() {
            Ok(stream) => stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => return Err(error),
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        let reply = match read_request(&stream) {
            Ok(request) => self.apply(request, runner),
            Err(error) => self.reply(None, runner, Some(error.to_string())),
        };
        // The controller can retry the same sequence if its connection closes
        // after a command was applied but before receipt of its acknowledgement.
        if let Err(error) = serde_json::to_writer(&mut stream, &reply)
            .map_err(io::Error::other)
            .and_then(|()| stream.write_all(b"\n"))
        {
            eprintln!("control reply failed: {error}");
        }
        Ok(())
    }

    fn apply(&mut self, request: Request, runner: &mut Runner) -> Reply {
        let expected = match &self.last {
            Some((last, reply)) if last == &request => return reply.clone(),
            Some((last, _)) => last.sequence.checked_add(1),
            None => Some(1),
        };
        let error = if request.schema_version != 1 || request.run_id != self.run_id {
            Some("unexpected schema version or run ID".into())
        } else if Some(request.sequence) != expected {
            Some(format!(
                "out-of-order sequence {}, expected {expected:?}",
                request.sequence
            ))
        } else {
            request
                .command()
                .map_err(Into::into)
                .and_then(|command| runner.command(command))
                .err()
                .map(|error| error.to_string())
        };
        let reply = self.reply(Some(request.sequence), runner, error);
        if reply.accepted {
            self.last = Some((request, reply.clone()));
        }
        reply
    }

    fn reply(&self, sequence: Option<u64>, runner: &Runner, error: Option<String>) -> Reply {
        Reply {
            schema_version: 1,
            run_id: self.run_id.clone(),
            sequence,
            accepted: error.is_none(),
            state: runner.lifecycle().as_str(),
            at_utc: runner.at().ok(),
            error,
        }
    }
}

fn read_request(stream: &TcpStream) -> Result<Request, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(stream.take(COMMAND_LIMIT));
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line)?;
    if line.last() != Some(&b'\n') {
        return Err("expected one newline-delimited control command of at most 8192 bytes".into());
    }
    Ok(serde_json::from_slice(&line)?)
}
