//! `pt init`, `pt import`, `pt env` and `pt secret`.

use crate::context::Context;
use crate::style;
use anyhow::{Context as _, Result, bail};
use clap::{Args, Subcommand};
use post_tester_core::Workspace;
use post_tester_core::import;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Folder to create the workspace in.
    #[arg(default_value = ".")]
    dir: PathBuf,
    /// Workspace name (default: the folder name).
    #[arg(long)]
    name: Option<String>,
}

pub fn init(args: InitArgs) -> Result<ExitCode> {
    std::fs::create_dir_all(&args.dir)?;
    let dir = args.dir.canonicalize()?;
    let name = args.name.unwrap_or_else(|| dir.file_name().map_or("Workspace".into(), |n| n.to_string_lossy().into_owned()));
    Workspace::init(&dir, &name)?;
    println!("{} Created workspace \"{name}\" in {}", style::green("✓"), dir.display());
    println!();
    println!("  post-tester.yaml        workspace settings");
    println!("  collections/            one request per YAML file");
    println!("  environments/local.yaml variables (baseUrl = https://httpbin.org)");
    println!();
    println!("Next:");
    println!("  pt send examples/get-ip     send the sample request");
    println!("  pt serve                    open the app");
    Ok(ExitCode::SUCCESS)
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// File to import, or `-` to read from stdin.
    file: PathBuf,
    /// Folder to import into (default: derived from the collection name).
    #[arg(long)]
    into: Option<String>,
}

pub async fn import(workspace: Option<&Path>, args: ImportArgs) -> Result<ExitCode> {
    let ctx = Context::open(workspace, true).await?;
    let text = if args.file.as_os_str() == "-" {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text
    } else {
        std::fs::read_to_string(&args.file).with_context(|| format!("cannot read {}", args.file.display()))?
    };
    let imported = match import::detect(&text) {
        Some(format) => import::import(&text, Some(format))?,
        None => match ctx.plugins.import(&text).await {
            Some(result) => result?,
            None => import::import(&text, None)?,
        },
    };
    let summary = imported.write_to(&ctx.workspace()?, args.into.as_deref().unwrap_or(""))?;
    println!("{} Imported {} request(s) into collections/{}/", style::green("✓"), summary.requests.len(), summary.folder);
    for id in &summary.requests {
        println!("  {id}");
    }
    for env in &summary.environments {
        println!("  environment: {env}");
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Folder id to export (default: the whole workspace).
    #[arg(default_value = "")]
    folder: String,
    /// The plugin exporter, e.g. `markdown-exporter/markdown`.
    #[arg(short = 't', long = "to")]
    exporter: String,
    /// Write to a file instead of stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

pub async fn export(workspace: Option<&Path>, args: ExportArgs) -> Result<ExitCode> {
    let ctx = Context::open(workspace, true).await?;
    let ws = ctx.workspace()?;
    let exporters: Vec<String> = ctx.plugins.export_targets().into_iter().map(|(id, _)| id).collect();
    if exporters.is_empty() {
        bail!(
            "no exporter plugins are installed. Create one with `pt plugin new exporter <name>`, then add it with `pt plugin install <dir>`"
        );
    }
    if !exporters.contains(&args.exporter) {
        bail!("unknown exporter `{}`. Installed exporters: {}", args.exporter, exporters.join(", "));
    }
    let tree = ws.tree()?;
    let Some(folder) = tree.find(&args.folder) else { bail!("folder `{}` does not exist", args.folder) };
    let requests = folder.request_ids().into_iter().map(|id| ws.read_request(&id).map(|r| (id, r))).collect::<Result<Vec<_>, _>>()?;
    let content = ctx.plugins.export(&args.exporter, &folder.name, &requests).await?;
    match args.output {
        Some(path) => {
            std::fs::write(&path, content).with_context(|| format!("cannot write {}", path.display()))?;
            println!("{} Exported {} request(s) to {}", style::green("✓"), requests.len(), path.display());
        }
        None => print!("{content}"),
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Debug, Subcommand)]
pub enum EnvCommand {
    /// List environments.
    List,
    /// Show the variables of an environment.
    Show { name: String },
}

pub async fn env(workspace: Option<&Path>, command: EnvCommand) -> Result<ExitCode> {
    let ctx = Context::open(workspace, true).await?;
    let ws = ctx.workspace()?;
    match command {
        EnvCommand::List => {
            let default = ws.settings()?.default_environment;
            for name in ws.environments()? {
                let marker = if default.as_deref() == Some(name.as_str()) { style::green(" (default)") } else { String::new() };
                println!("{name}{marker}");
            }
        }
        EnvCommand::Show { name } => {
            for (key, value) in ws.read_environment(&name)?.variables {
                println!("{key} = {}", post_tester_core::model::value_to_string(&value));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Debug, Subcommand)]
pub enum SecretCommand {
    /// Store a secret. Reads the value from stdin unless --value is given.
    Set {
        name: String,
        /// The value (visible in shell history; prefer stdin).
        #[arg(long)]
        value: Option<String>,
    },
    /// Delete a secret.
    Delete { name: String },
}

pub async fn secret(workspace: Option<&Path>, command: SecretCommand) -> Result<ExitCode> {
    let ctx = Context::open(workspace, false).await?;
    let secrets = ctx.engine.secrets();
    match command {
        SecretCommand::Set { name, value } => {
            let value = match value {
                Some(value) => value,
                None => {
                    eprintln!("Value for `{name}` (then Enter):");
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim_end_matches(['\n', '\r']).to_string()
                }
            };
            if value.is_empty() {
                bail!("the value is empty; nothing was stored");
            }
            secrets.set(&name, &value).await?;
            println!("{} Stored `{name}` in the {}. Use it as {{{{secret:{name}}}}}", style::green("✓"), secrets.name());
        }
        SecretCommand::Delete { name } => {
            secrets.delete(&name).await?;
            println!("{} Deleted `{name}`", style::green("✓"));
        }
    }
    Ok(ExitCode::SUCCESS)
}
