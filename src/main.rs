mod bootstrap;

use std::process::ExitCode;

fn main() -> ExitCode {
    bootstrap::run(std::env::args_os())
}
