//! `sshcraft-server`: the sync server and its operator commands.

use clap::{Parser, Subcommand};
use sshcraft_server::admin;
use std::process::ExitCode;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "sshcraft-server",
    version,
    about = "Self-hosted sync server for SSHCraft"
)]
struct Cli {
    /// Database: sqlite://<path> or postgres://user:password@host/database
    #[arg(
        long,
        env = "SSHCRAFT_DATABASE_URL",
        hide_env_values = true,
        default_value = "sqlite://sshcraft.db",
        global = true
    )]
    database_url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage users
    Users {
        #[command(subcommand)]
        action: Users,
    },
    /// Manage the devices of a user
    Devices {
        #[command(subcommand)]
        action: Devices,
    },
    /// Show or rotate the API key new users and new devices need
    ApiKey {
        #[command(subcommand)]
        action: ApiKey,
    },
}

#[derive(Subcommand)]
enum Users {
    /// List all users
    List,
    /// Lock a user: no login, no changes to areas they own
    Lock { email: String },
    /// Unlock a user
    Unlock { email: String },
    /// Delete a user; without --yes only shows what happens to their areas
    Delete {
        email: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum Devices {
    /// List the devices of a user
    List { email: String },
    /// Block a device on this server (the user still has to revoke it to rotate keys)
    Block { email: String, device_id: Uuid },
    /// Lift a block
    Unblock { email: String, device_id: Uuid },
}

#[derive(Subcommand)]
enum ApiKey {
    /// Print the API key
    Show,
    /// Create a new API key; devices that logged in before keep working
    Rotate,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let store = sshcraft_server::store::open(&cli.database_url).await?;
    let store = store.as_ref();
    let out = &mut std::io::stdout().lock();
    match cli.command {
        Command::Users { action } => match action {
            Users::List => admin::users_list(store, out).await?,
            Users::Lock { email } => admin::users_lock(store, &email, true, out).await?,
            Users::Unlock { email } => admin::users_lock(store, &email, false, out).await?,
            Users::Delete { email, yes } => admin::users_delete(store, &email, yes, out).await?,
        },
        Command::Devices { action } => match action {
            Devices::List { email } => admin::devices_list(store, &email, out).await?,
            Devices::Block { email, device_id } => {
                admin::devices_block(store, &email, device_id, true, out).await?
            }
            Devices::Unblock { email, device_id } => {
                admin::devices_block(store, &email, device_id, false, out).await?
            }
        },
        Command::ApiKey { action } => match action {
            ApiKey::Show => admin::api_key_show(store, out).await?,
            ApiKey::Rotate => admin::api_key_rotate(store, out).await?,
        },
    }
    Ok(())
}
