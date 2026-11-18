mod catalog;
mod mission;

use std::{env, process::ExitCode};

const BANNER: &str = "HOTH — High-Throughput Orbit Telemetry Harness";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => println!("{BANNER}"),
        [arg] if arg == "-h" || arg == "--help" => println!(
            "{BANNER}\n\nUsage: hoth [OPTIONS]\n\nCommands:\n  catalog bootstrap, import, metadata, snapshot, validate, stats, export\n  scenario prepare --config PATH --output PATH [--database PATH] [--entity-types PATH]\n  run --manifest PATH [--control IP:PORT] [--run-id ID] [--ready-file PATH]\n  health --ready-file PATH\n\nOptions:\n  -h, --help     Show help\n  -V, --version  Show version"
        ),
        [arg] if arg == "-V" || arg == "--version" => {
            println!("hoth {}", env!("CARGO_PKG_VERSION"));
        }
        [command, subcommand, rest @ ..] if command == "catalog" && subcommand == "bootstrap" => {
            return catalog::bootstrap(rest);
        }
        [command, subcommand, rest @ ..] if command == "catalog" => {
            return result(catalog::offline(subcommand.to_str().unwrap_or_default(), rest));
        }
        [command, subcommand, rest @ ..] if command == "scenario" && subcommand == "prepare" => {
            return result(mission::prepare(rest));
        }
        [command] if command == "run" => {
            eprintln!("error: run requires --manifest. Run 'hoth --help' for usage.");
            return ExitCode::from(2);
        }
        [command, rest @ ..] if command == "run" => return result(mission::run(rest)),
        [command, rest @ ..] if command == "health" => return result(mission::health(rest)),
        _ => {
            eprintln!("error: unsupported arguments. Run 'hoth --help' for usage.");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

fn result(result: Result<(), Box<dyn std::error::Error>>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
