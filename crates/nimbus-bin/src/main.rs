use std::error::Error;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt::init();
    match nimbus_cli::run_from_env().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprint!("{}", format_operator_error(error.as_ref()));
            ExitCode::FAILURE
        }
    }
}

fn format_operator_error(error: &dyn Error) -> String {
    use std::fmt::Write as _;

    let mut rendered = format!("error: {error}\n");
    let mut source = error.source();
    while let Some(cause) = source {
        writeln!(rendered, "  caused by: {cause}").expect("writing to a String cannot fail");
        source = cause.source();
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    #[derive(Debug)]
    struct WrappedError {
        source: std::io::Error,
    }

    impl fmt::Display for WrappedError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("could not start Nimbus")
        }
    }

    impl Error for WrappedError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.source)
        }
    }

    #[test]
    fn operator_error_keeps_the_source_chain_without_debug_text() {
        let error = WrappedError {
            source: std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "data directory is not writable",
            ),
        };

        assert_eq!(
            format_operator_error(&error),
            "error: could not start Nimbus\n  caused by: data directory is not writable\n"
        );
    }
}
