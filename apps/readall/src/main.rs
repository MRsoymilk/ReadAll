use std::{
    error::Error,
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use readall_core::{Limits, TextDocument};
use readall_platform::LocalFileSource;

fn main() -> ExitCode {
    match run(
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

fn run(args: Vec<OsString>, output: &mut impl Write) -> Result<(), Box<dyn Error>> {
    if args.is_empty() || matches!(args[0].to_str(), Some("--help" | "-h")) {
        writeln!(
            output,
            "ReadAll {} — native Rust reader foundations\n\nUsage: readall inspect <book.txt>\n\nDiagnostic CLI only. Native GUI, PDF and EPUB reading are not implemented yet.",
            env!("CARGO_PKG_VERSION")
        )?;
        return Ok(());
    }
    if args.len() != 2 || args[0] != "inspect" {
        return Err("expected: readall inspect <book.txt>".into());
    }
    let mut source = LocalFileSource::open(PathBuf::from(&args[1]))?;
    let document = TextDocument::open(&mut source, Limits::default())?;
    writeln!(
        output,
        "Format: TXT\nEncoding: {:?}\nDocument: {}\nUTF-8 bytes: {}\nUnicode scalars: {}\nStart locator: {}",
        document.encoding(),
        document.id(),
        document.text().len(),
        document.text().chars().count(),
        document.locator(0)?
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn help_does_not_claim_gui_or_pdf_support() {
        let mut output = Vec::new();
        run(vec![], &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("inspect"));
        assert!(text.contains("not implemented"));
    }
    #[test]
    fn invalid_arguments_are_rejected() {
        assert!(run(vec!["inspect".into()], &mut Vec::new()).is_err());
        assert!(run(vec!["unknown".into(), "book.txt".into()], &mut Vec::new()).is_err());
    }
}
