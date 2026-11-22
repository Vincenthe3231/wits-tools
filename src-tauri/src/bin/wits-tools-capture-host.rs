//! Bridges browser native messaging to the WITS Tools capture session.

fn main() -> std::process::ExitCode {
    match wits_tools_lib::crawler::capture::native_host() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}
