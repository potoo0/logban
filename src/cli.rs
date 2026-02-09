use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    version = concat!(
        env!("CARGO_PKG_VERSION"), " - ",
        env!("VERGEN_GIT_DESCRIBE"), "(",
        env!("VERGEN_BUILD_DATE"), ")"
    ),
    about
)]
pub struct Args {
    #[arg(short, long, default_value = "config.yaml")]
    pub config: String,
    #[arg(short = 'n', long, default_value_t = false)]
    pub dry_run: bool,
    #[arg(
        short = 'l',
        long,
        help = "Set the log level (overrides env var), eg: info,logban=trace"
    )]
    pub log_level: Option<String>,
}
