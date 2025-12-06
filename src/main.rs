mod config;
mod jenkins;
mod monitor;
mod alert;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "jenkins-monitor")]
#[command(about = "Monitor Jenkins jobs and alert when they don't run as expected")]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    let cli = Cli::parse();
    
    log::info!("Starting Jenkins Monitor");
    log::info!("Loading configuration from: {:?}", cli.config);
    
    let config = config::Config::load(&cli.config)?;
    log::info!("Configuration loaded successfully");
    
    let jenkins_client = jenkins::JenkinsClient::new(&config.jenkins)?;
    log::info!("Jenkins client initialized for: {}", config.jenkins.url);
    
    let monitor = monitor::Monitor::new(config, jenkins_client);
    
    log::info!("Starting monitoring loop...");
    monitor.run().await?;
    
    Ok(())
}
