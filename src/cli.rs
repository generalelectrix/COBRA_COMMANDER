use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use rust_dmx::{DmxPort, OfflineDmxPort, available_ports};
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
pub(crate) fn run_cli_configuration(client: CommandClient, universe_count: usize) -> Result<()> {
    offer_action(&client, |c| prompt_assign_dmx_ports(c, universe_count))?;
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
    action: impl Fn(&CommandClient) -> Result<Option<CommandResponse>>,
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

const ARTNET_POLL_TIMEOUT: Duration = Duration::from_secs(10);

fn prompt_assign_dmx_ports(
    client: &CommandClient,
    universe_count: usize,
) -> Result<Option<CommandResponse>> {
    if !prompt_bool("Assign DMX ports?")? {
        return Ok(None);
    }
    let artnet = prompt_bool("Scan for artnet ports?")?;
    let artnet_timeout = artnet.then_some(ARTNET_POLL_TIMEOUT);
    if artnet {
        println!("Searching for artnet ports...");
    }
    let mut ports = available_ports(artnet_timeout)?;
    for universe in 0..universe_count {
        println!("Assign port to universe {universe}:");
        let port = prompt_select_port(&mut ports)?;
        let response = client.send_command(MetaCommand::AssignDmxPort { universe, port })?;
        if let Err(e) = &response {
            println!("Error assigning universe {universe}: {e}");
        }
    }
    Ok(Some(Ok(())))
}

/// Prompt the user to select a DMX port. Does NOT open the port.
///
/// Selected ports are removed from the list so they can't be double-assigned.
fn prompt_select_port(ports: &mut Vec<Box<dyn DmxPort>>) -> Result<Box<dyn DmxPort>> {
    println!("Available DMX ports:");
    println!("  0: offline");
    for (i, port) in ports.iter().enumerate() {
        println!("  {}: {port}", i + 1);
    }
    loop {
        print!("Select a port: ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let input = tunnels_lib::prompt::read_string()?;
        let index: usize = match input.trim().parse() {
            Ok(v) => v,
            Err(e) => {
                println!("{e}");
                continue;
            }
        };
        if index == 0 {
            return Ok(Box::new(OfflineDmxPort) as Box<dyn DmxPort>);
        }
        let index = index - 1;
        if index >= ports.len() {
            println!("please enter a value less than {}", ports.len() + 1);
            continue;
        }
        return Ok(ports.remove(index));
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
