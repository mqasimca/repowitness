//! RepoWitness command-line interface and process composition root.

use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let stderr = io::stderr();
    let code = if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("mcp-serve")) {
        repowitness_cli::run_mcp_server(std::env::args_os(), stderr.lock())
    } else {
        let stdout = io::stdout();
        repowitness_cli::run(std::env::args_os(), stdout.lock(), stderr.lock())
    };
    ExitCode::from(code)
}
