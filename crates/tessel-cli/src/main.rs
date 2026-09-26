use std::process::ExitCode;

const USAGE: &str = "usage: tessel <command>

commands:
  check <file.tsl>   check a program for errors (not implemented yet)
  build <file.tsl>   compile a program to a native app (not implemented yet)
  run <file.tsl>     build and run a program (not implemented yet)
  version            print the compiler version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("version" | "--version" | "-V") => {
            println!("tessel {}", tessel_compiler::VERSION);
            ExitCode::SUCCESS
        }
        Some(cmd @ ("check" | "build" | "run")) => {
            eprintln!("tessel: `{cmd}` is not implemented yet");
            ExitCode::FAILURE
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}
