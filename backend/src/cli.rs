use std::io::{self, Read};

use anyhow::{bail, Context, Result};

use crate::{auth, config::Config, db::Db, error::AppError};

pub enum CliAction {
    Handled,
    RunServer,
}

pub async fn handle_args() -> Result<CliAction> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return Ok(CliAction::RunServer);
    };

    match command.as_str() {
        "admin" => handle_admin_args(args.collect()).await,
        "-h" | "--help" | "help" => {
            print_help();
            Ok(CliAction::Handled)
        }
        other => bail!("unknown command: {other}"),
    }
}

async fn handle_admin_args(args: Vec<String>) -> Result<CliAction> {
    let Some(subcommand) = args.first() else {
        print_admin_help();
        return Ok(CliAction::Handled);
    };

    match subcommand.as_str() {
        "reset-password" => {
            let options = ResetPasswordOptions::parse(&args[1..])?;
            reset_admin_password(options).await?;
            Ok(CliAction::Handled)
        }
        "-h" | "--help" | "help" => {
            print_admin_help();
            Ok(CliAction::Handled)
        }
        other => bail!("unknown admin command: {other}"),
    }
}

#[derive(Debug)]
struct ResetPasswordOptions {
    username: String,
    password_stdin: bool,
}

impl ResetPasswordOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut username = "admin".to_string();
        let mut password_stdin = false;
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--username" | "-u" => {
                    index += 1;
                    username = args
                        .get(index)
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                        .context("--username requires a non-empty value")?;
                }
                "--password-stdin" => {
                    password_stdin = true;
                }
                "-h" | "--help" => {
                    print_reset_password_help();
                    std::process::exit(0);
                }
                other => bail!("unknown reset-password option: {other}"),
            }
            index += 1;
        }

        Ok(Self {
            username,
            password_stdin,
        })
    }
}

async fn reset_admin_password(options: ResetPasswordOptions) -> Result<()> {
    let config = Config::from_env()?;
    let db = Db::connect(&config).await?;
    let password = read_new_password(options.password_stdin)?;
    auth::validate_user_password_input(&password).map_err(app_error_to_anyhow)?;
    let password_hash = auth::hash_user_password(&password, &config.admin_token_secret);

    let result = sqlx::query(
        r#"
        UPDATE admin
        SET password_hash = $2,
            status = 'enabled',
            failed_login_attempts = 0,
            locked_until = NULL,
            password_changed_at = now(),
            updated_at = now()
        WHERE username = $1
        RETURNING id
        "#,
    )
    .bind(&options.username)
    .bind(password_hash)
    .fetch_optional(&db.pool)
    .await?;

    if result.is_none() {
        bail!("admin user '{}' was not found", options.username);
    }

    println!(
        "Admin password reset for '{}'. The account is enabled and login lock state was cleared.",
        options.username
    );
    Ok(())
}

fn read_new_password(password_stdin: bool) -> Result<String> {
    if password_stdin {
        let mut password = String::new();
        io::stdin()
            .read_to_string(&mut password)
            .context("failed to read password from stdin")?;
        return Ok(password.trim_end_matches(['\r', '\n']).to_string());
    }

    let password = rpassword::prompt_password("New admin password: ")?;
    let confirmation = rpassword::prompt_password("Confirm new admin password: ")?;
    if password != confirmation {
        bail!("passwords do not match");
    }
    Ok(password)
}

fn app_error_to_anyhow(err: AppError) -> anyhow::Error {
    anyhow::anyhow!("{err}")
}

fn print_help() {
    println!(
        "NeoGate\n\nUsage:\n  neogate [admin reset-password]\n\nCommands:\n  admin reset-password    Reset an administrator password"
    );
}

fn print_admin_help() {
    println!(
        "NeoGate admin commands\n\nUsage:\n  neogate admin reset-password [--username admin] [--password-stdin]"
    );
}

fn print_reset_password_help() {
    println!(
        "Reset an administrator password\n\nUsage:\n  neogate admin reset-password [--username admin] [--password-stdin]\n\nOptions:\n  -u, --username <name>    Administrator username to reset (default: admin)\n      --password-stdin     Read the new password from stdin"
    );
}
