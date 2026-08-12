use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use vanityctl::{ConfigPaths, Manager};

#[derive(Parser)]
#[command(name = "hostd", version, about = "vanityctl control-plane daemon")]
struct Args {
    /// Path to config.yaml (also available as VANITYCTL_CONFIG)
    #[arg(long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hostd=info".into()),
        )
        .init();
    let args = Args::parse();
    if let Some(path) = args.config {
        unsafe {
            std::env::set_var("VANITYCTL_CONFIG", path);
        }
    }
    let manager = Arc::new(Manager::system(ConfigPaths::discover()?)?);
    vanityctl::api::serve(manager).await
}
