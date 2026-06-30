use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, error::ErrorKind};
use tempfile::tempdir;

use super::adapter::detect_dev_adapter;
use super::banner::dev_banner_lines;
use super::firebase::{firestore_wiring_refusal_lines, wire_firestore_client_app};
use super::firebase_scan::CoveredSet;
use super::launch::{AutoOpenDecision, EnvLookup, resolve_auto_open};
use super::plan::{DevPlan, detect_app_dir};
use super::watch::collect_source_snapshot;
use super::*;
use crate::start::{CliTenantProvider, StartCommand};
use crate::test_support::with_current_dir;
use crate::{Cli, Command};

mod adapter_detection;
mod adoption;
mod banner;
mod cli_surface;
mod env_local;
mod firestore_wiring;
mod plan;

fn parse_dev<I, T>(args: I) -> DevCommand
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    let Command::Dev(command) = cli.command else {
        panic!("dev subcommand should parse");
    };
    *command
}

fn create_source_root(app_dir: &Path, root: &str) {
    fs::create_dir_all(app_dir.join(root)).expect("source root should build");
}
