use clap::Parser;

#[derive(Parser)]
#[command(about)]
pub(crate) struct Cli {
    /// If true, provide verbose logging.
    #[arg(long)]
    pub debug: bool,
}
