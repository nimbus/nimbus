use std::path::PathBuf;

use nimbus_core::{Error, Result};

const DEFAULT_FULL_SAMPLES: usize = 3;
const QUICK_FULL_SAMPLES: usize = 1;

#[derive(Debug)]
pub(super) struct Arguments {
    pub(super) output: Option<PathBuf>,
    pub(super) quick: bool,
    pub(super) candidate_only: bool,
    pub(super) documents: Option<usize>,
    pub(super) payload_bytes: Option<usize>,
    pub(super) full_samples: usize,
    pub(super) child: Option<ChildArguments>,
}

#[derive(Debug)]
pub(super) struct ChildArguments {
    pub(super) data_dir: PathBuf,
    pub(super) documents: usize,
    pub(super) payload_bytes: usize,
    pub(super) churn_basis_points: u32,
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn parse_arguments() -> Result<Arguments> {
    parse_arguments_from(std::env::args().skip(1))
}

pub(super) fn parse_arguments_from(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Arguments> {
    let mut output = None;
    let mut quick = false;
    let mut candidate_only = false;
    let mut documents = None;
    let mut payload_bytes = None;
    let mut full_samples = DEFAULT_FULL_SAMPLES;
    let mut full_samples_explicit = false;
    let mut child_data_dir = None;
    let mut child_churn = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--bench" => {}
            "--quick" => {
                quick = true;
                full_samples = QUICK_FULL_SAMPLES;
            }
            "--candidate-only" => candidate_only = true,
            "--output" => output = Some(PathBuf::from(next_value(&mut arguments, "--output")?)),
            "--documents" => {
                documents = Some(parse_usize(
                    next_value(&mut arguments, "--documents")?,
                    "--documents",
                )?)
            }
            "--payload-bytes" => {
                payload_bytes = Some(parse_usize(
                    next_value(&mut arguments, "--payload-bytes")?,
                    "--payload-bytes",
                )?)
            }
            "--full-samples" => {
                full_samples_explicit = true;
                full_samples = parse_usize(
                    next_value(&mut arguments, "--full-samples")?,
                    "--full-samples",
                )?
            }
            "--child-full" => {
                child_data_dir = Some(PathBuf::from(next_value(&mut arguments, "--child-full")?))
            }
            "--churn-basis-points" => {
                child_churn = Some(parse_u32(
                    next_value(&mut arguments, "--churn-basis-points")?,
                    "--churn-basis-points",
                )?)
            }
            _ => {
                return Err(Error::InvalidInput(format!(
                    "unknown materialized-verification argument: {argument}"
                )));
            }
        }
    }
    if full_samples == 0 {
        return Err(Error::InvalidInput(
            "--full-samples must be positive".to_string(),
        ));
    }
    if candidate_only
        && (quick
            || documents.is_some()
            || payload_bytes.is_some()
            || full_samples_explicit
            || child_data_dir.is_some()
            || child_churn.is_some())
    {
        return Err(Error::InvalidInput(
            "--candidate-only accepts only --output".to_string(),
        ));
    }
    let child = match child_data_dir {
        Some(data_dir) => Some(ChildArguments {
            data_dir,
            documents: documents.ok_or_else(|| {
                Error::InvalidInput("--child-full requires --documents".to_string())
            })?,
            payload_bytes: payload_bytes.ok_or_else(|| {
                Error::InvalidInput("--child-full requires --payload-bytes".to_string())
            })?,
            churn_basis_points: child_churn.ok_or_else(|| {
                Error::InvalidInput("--child-full requires --churn-basis-points".to_string())
            })?,
        }),
        None => None,
    };
    Ok(Arguments {
        output,
        quick,
        candidate_only,
        documents,
        payload_bytes,
        full_samples,
        child,
    })
}

fn next_value(arguments: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    arguments
        .next()
        .ok_or_else(|| Error::InvalidInput(format!("{flag} requires a value")))
}

fn parse_usize(value: String, flag: &str) -> Result<usize> {
    value
        .parse()
        .map_err(|error| Error::InvalidInput(format!("invalid {flag}: {error}")))
}

fn parse_u32(value: String, flag: &str) -> Result<u32> {
    value
        .parse()
        .map_err(|error| Error::InvalidInput(format!("invalid {flag}: {error}")))
}
