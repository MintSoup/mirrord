//! The `mirrord up` command - runs multiple mirrord sessions from a `mirrord-up.yaml` file.

use mirrord_up::load_up_config;

use crate::{config::UpArgs, error::CliResult};

/// The `mirrord up` command handler.
pub(crate) async fn up_command(args: UpArgs) -> CliResult<()> {
    let up_config = load_up_config(&args.config_file).map_err(crate::error::CliError::Up)?;

    // MVP: log services and exit cleanly
    tracing::info!(
        "Loaded mirrord-up config from {}",
        args.config_file.display()
    );
    for name in up_config.services.keys() {
        tracing::info!("Found service: {}", name);
    }

    Ok(())
}
