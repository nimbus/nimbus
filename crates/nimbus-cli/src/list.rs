use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::function_scaling::{known_function_selectors, load_config};

#[derive(Debug, Args)]
pub(crate) struct ListCommand {
    #[command(subcommand)]
    resource: ListResource,
}

#[derive(Debug, Subcommand)]
enum ListResource {
    /// List known function selectors from nimbus.yaml overrides.
    Functions(ListFunctionsCommand),
}

#[derive(Debug, Args)]
struct ListFunctionsCommand {
    /// Path to nimbus.yaml / nimbus.json.
    #[arg(long)]
    config: Option<PathBuf>,
}

pub(crate) async fn run_list_command(command: ListCommand) -> nimbus::Result<()> {
    match command.resource {
        ListResource::Functions(command) => run_list_functions(command),
    }
}

fn run_list_functions(command: ListFunctionsCommand) -> nimbus::Result<()> {
    let config = load_config(command.config)?;
    let names = known_function_selectors(&config.functions.scaling);
    if names.is_empty() {
        println!("No function overrides configured.");
    } else {
        for name in names {
            println!("{name}");
        }
    }
    Ok(())
}
