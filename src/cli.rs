use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use strum_macros::Display;
use tunnels_lib::prompt::prompt_bool;

use crate::control::{CommandClient, CommandResponse, MetaCommand};

#[derive(Parser)]
#[command(about)]
pub(crate) struct Cli {
    /// If true, provide verbose logging.
    #[arg(long)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Display)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum Command {
    /// Run the controller.
    Run(RunArgs),

    /// Check that the provided patch file is valid, then quit.
    Check(CheckArgs),

    /// Run the animation visualizer.
    Viz,

    /// Get fixture info.
    Fix(FixArgs),
}

#[derive(Args)]
pub(crate) struct RunArgs {
    /// Path to a YAML file containing the fixture patch.
    pub patch_file: PathBuf,

    /// If true, speedrun auto configuration with defaults.
    ///
    /// Mostly useful for testing.
    #[arg(long)]
    pub quickstart: bool,

    /// If true, poll for artnet interfaces as possible DMX ports.
    #[arg(long)]
    pub artnet: bool,

    /// If true, use the last channel fader as a master strobe control.
    #[arg(long)]
    pub master_strobe_channel: bool,

    /// The port on which to listen for OSC messages.
    #[arg(long, default_value_t = 8000)]
    pub osc_receive_port: u16,

    /// If true, render fixture preview into the CLI.
    #[arg(long)]
    pub cli_preview: bool,
}

#[derive(Args)]
pub(crate) struct CheckArgs {
    /// Path to a YAML file containing the fixture patch.
    pub patch_file: PathBuf,

    /// Print the OSC controls for each fixture group in the patch.
    #[arg(long)]
    pub describe_controls: bool,
}

#[derive(Args)]
pub(crate) struct FixArgs {
    /// Show info for all registered fixture types.
    #[arg(long)]
    pub all: bool,

    /// Show info for one fixture type.
    pub fixture: Option<String>,
}

/// Interactive CLI configuration, running against a live show.
pub(crate) fn run_cli_configuration(client: CommandClient) -> Result<()> {
    offer_action(&client, prompt_start_animation_visualizer)?;
    Ok(())
}

/// Run a user-facing action that may produce a command.
///
/// Calls `action` to prompt the user and build a command. If the command
/// fails, the user is offered the chance to try again (re-running the
/// full prompt flow) or move on. If `action` returns `Ok(None)`, the
/// user declined and we move on.
fn offer_action(
    client: &CommandClient,
    action: fn(&CommandClient) -> Result<Option<CommandResponse>>,
) -> Result<()> {
    loop {
        match action(client)? {
            None | Some(Ok(())) => return Ok(()),
            Some(Err(e)) => {
                println!("Error: {e}");
                if !prompt_bool("Try again?")? {
                    return Ok(());
                }
            }
        }
    }
}

fn prompt_start_animation_visualizer(client: &CommandClient) -> Result<Option<CommandResponse>> {
    if !prompt_bool("Start animation visualizer?")? {
        return Ok(None);
    }
    Ok(Some(
        client.send_command(MetaCommand::StartAnimationVisualizer)?,
    ))
}
