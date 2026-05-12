use std::io::{Read, Write};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::sync::{Mutex, OnceLock};

use super::*;
use crate::machine::manager::MachineHelperEnvGuard;
use clap::{Parser, Subcommand, error::ErrorKind};
use tempfile::TempDir;

mod forwarded_api;
mod os_image;
mod parse_help;
mod records_state;
mod render;
mod startup_failures;
mod support;
mod transfer_ssh;

use self::support::*;
