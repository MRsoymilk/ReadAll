mod cli;
mod text_page;

use std::{io, process::ExitCode};

fn main() -> ExitCode {
    match cli::run(
        std::env::args_os().skip(1).collect(),
        &mut io::stdout().lock(),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
            {
                return ExitCode::SUCCESS;
            }
            eprintln!("ReadAll: {error}");
            ExitCode::FAILURE
        }
    }
}
