//! The `mirrord up` command - runs multiple mirrord sessions from a `mirrord-up.yaml` file.

use std::process::Stdio;

use mirrord_config::{LayerConfig, config::EnvKey};
use mirrord_progress::MIRRORD_PROGRESS_ENV;
use mirrord_up::{SubprocessCfg, load_up_config};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, BufReader},
    process::Command,
    task::JoinSet,
};

use crate::{CliError, config::UpArgs, error::CliResult};

pub const RESOLVED_CONFIG_ENV: &str = "MIRRORD_UP_RESOLVED_CONFIG";

/// The `mirrord up` command handler.
pub(crate) async fn up_command(args: UpArgs) -> CliResult<()> {
    let up_config = load_up_config(&args.config_file).map_err(crate::error::CliError::Up)?;
    let key = args
        .key
        .map(EnvKey::Provided)
        .unwrap_or_else(|| EnvKey::Generated(whoami::username().unwrap()));

    let commands: Vec<_> = up_config
        .service_configs(&key)
        .map(|config| {
            let SubprocessCfg {
                config,
                service_name,
                run,
            } = config;

            let encoded_cfg = config.encode()?;

            let mut cmd = Command::new(std::env::args().next().unwrap());
            cmd.env(RESOLVED_CONFIG_ENV, encoded_cfg)
                .env(MIRRORD_PROGRESS_ENV, "json")
                .arg(Into::<&'static str>::into(run.r#type))
                .arg("--")
                .args(run.command)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            Ok((service_name, cmd))
        })
        .collect::<Result<_, CliError>>()?;

    let mut handles = JoinSet::new();
    for (name, mut command) in commands {
        let mut child = command.spawn().unwrap();
        println!("Running {name}: {child:?}");
        handles.spawn(async move {
            let mut err = BufReader::new(child.stderr.take().unwrap()).lines();
            let mut out = BufReader::new(child.stdout.take().unwrap()).lines();

            loop {
                tokio::select! {
                    line = out.next_line() => match line {
                        Ok(Some(line)) => println!("{name}: {line}"),
                        Ok(None) => {}
                        Err(err) => println!("{name} error: {err:?}"),
                    },

                    line = err.next_line() => match line {
                        Ok(Some(line)) => println!("{name}: {line}"),
                        Ok(None) => {}
                        Err(err) => println!("{name} error: {err:?}"),
                    },

                    done = child.wait() => {
                        println!("{done:?}");
                        break;
                    }

                }
            }
        });
    }

    handles.join_all().await;

    Ok(())
}
