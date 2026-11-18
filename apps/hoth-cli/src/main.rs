use std::{env, process::ExitCode};

const BANNER: &str = "HOTH — High-Throughput Orbit Telemetry Harness (foundation scaffold)";

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    match (args.next(), args.next()) {
        (None, None) => println!("{BANNER}"),
        (Some(arg), None) if arg == "-h" || arg == "--help" => println!(
            "{BANNER}\n\nUsage: hoth [OPTIONS]\n\nOptions:\n  -h, --help     Show help\n  -V, --version  Show version"
        ),
        (Some(arg), None) if arg == "-V" || arg == "--version" => {
            println!("hoth {}", env!("CARGO_PKG_VERSION"));
        }
        _ => {
            eprintln!("error: unsupported arguments. Run 'hoth --help' for usage.");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
