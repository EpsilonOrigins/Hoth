mod catalog;

use std::{env, process::ExitCode};

const BANNER: &str = "HOTH — High-Throughput Orbit Telemetry Harness (foundation scaffold)";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => println!("{BANNER}"),
        [arg] if arg == "-h" || arg == "--help" => println!(
            "{BANNER}\n\nUsage: hoth [OPTIONS]\n\nCommands:\n  catalog bootstrap  Import historical Space-Track elements\n\nOptions:\n  -h, --help     Show help\n  -V, --version  Show version"
        ),
        [arg] if arg == "-V" || arg == "--version" => {
            println!("hoth {}", env!("CARGO_PKG_VERSION"));
        }
        [command, subcommand, rest @ ..] if command == "catalog" && subcommand == "bootstrap" => {
            return catalog::bootstrap(rest);
        }
        _ => {
            eprintln!("error: unsupported arguments. Run 'hoth --help' for usage.");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
