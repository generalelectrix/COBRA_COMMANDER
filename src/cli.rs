use clap::Parser;

#[derive(Parser)]
#[command(about)]
pub(crate) struct Cli {
    /// If true, provide verbose logging.
    #[arg(long)]
    pub debug: bool,

    /// The port on which to listen for OSC messages.
    #[arg(long, default_value_t = 8000)]
    pub osc_receive_port: u16,
}
