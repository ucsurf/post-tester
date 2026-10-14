//! `pt plugin …`: create, validate, test, install and approve plugins.

use crate::context::Context;
use crate::style;
use anyhow::{Context as _, Result, bail};
use clap::Subcommand;
use post_tester_plugin_host::manifest::{Manifest, PLUGIN_TYPES, Permission, Problem};
use post_tester_plugin_host::scaffold;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Create a plugin from a template.
    New {
        /// template-function, auth, hook, importer, exporter, codegen, viewer or mcp-tool.
        kind: String,
        /// Plugin name, e.g. `totp-generator`.
        name: String,
        /// Where to create it (default: ./<name>).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Check plugin.json and the code, and list every problem with a fix.
    Validate {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Run the plugin's *.test.ts files.
    Test {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Add a plugin to the workspace and review its permissions.
    Install {
        dir: PathBuf,
        /// Approve all requested permissions without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Approve the permissions a plugin asks for.
    Approve { name: String },
    /// Remove a plugin from the workspace (its files are kept).
    Remove { name: String },
    /// List the workspace's plugins.
    List,
}

pub async fn plugin(workspace: Option<&Path>, command: PluginCommand) -> Result<ExitCode> {
    match command {
        PluginCommand::New { kind, name, dir } => new(&kind, &name, dir),
        PluginCommand::Validate { dir } => validate(workspace, &dir).await,
        PluginCommand::Test { dir } => test(workspace, &dir).await,
        PluginCommand::Install { dir, yes } => install(workspace, &dir, yes).await,
        PluginCommand::Approve { name } => approve(workspace, &name).await,
        PluginCommand::Remove { name } => remove(workspace, &name).await,
        PluginCommand::List => list(workspace).await,
    }
}

fn new(kind: &str, name: &str, dir: Option<PathBuf>) -> Result<ExitCode> {
    let dir = dir.unwrap_or_else(|| PathBuf::from(name));
    let files = scaffold::create(&dir, kind, name).map_err(anyhow::Error::msg)?;
    println!("{} Created {kind} plugin `{name}` in {}", style::green("✓"), dir.display());
    for file in files {
        println!("  {file}");
    }
    println!();
    println!("Next:");
    println!("  cd {}", dir.display());
    println!("  pt plugin test          run the example test");
    println!("  pt plugin install .     add it to your workspace");
    println!();
    println!("Guide: guide/plugins/README.md · Types: packages/plugin-sdk/index.d.ts");
    Ok(ExitCode::SUCCESS)
}

fn print_problems(problems: &[Problem]) {
    for p in problems {
        println!("{} {}: {}", style::red("✗"), style::bold(&p.location), p.message);
        println!("    {} {}", style::dimmed("fix:"), p.fix);
    }
}

async fn validate(workspace: Option<&Path>, dir: &Path) -> Result<ExitCode> {
    let ctx = Context::open(workspace, false).await?;
    let problems = ctx.plugins.validate(dir).await;
    if problems.is_empty() {
        println!("{} {} is valid", style::green("✓"), dir.display());
        Ok(ExitCode::SUCCESS)
    } else {
        print_problems(&problems);
        println!("\n{} problem(s) found", problems.len());
        Ok(ExitCode::from(1))
    }
}

async fn test(workspace: Option<&Path>, dir: &Path) -> Result<ExitCode> {
    let ctx = Context::open(workspace, false).await?;
    match ctx.plugins.test(dir).await {
        Ok(report) => {
            for test in &report.tests {
                if test.passed {
                    println!("{} {}", style::green("✓"), test.name);
                } else {
                    println!("{} {}", style::red("✗"), test.name);
                    if let Some(detail) = &test.detail {
                        println!("    {}", style::red(detail));
                    }
                }
            }
            let failed = report.tests.iter().filter(|t| !t.passed).count();
            println!("\n{} tests, {} failed ({})", report.tests.len(), failed, report.files.join(", "));
            Ok(if report.passed() { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Err(problems) => {
            print_problems(&problems);
            Ok(ExitCode::from(1))
        }
    }
}

async fn install(workspace: Option<&Path>, dir: &Path, yes: bool) -> Result<ExitCode> {
    let ctx = Context::open(workspace, true).await?;
    let ws = ctx.workspace()?;
    let problems = ctx.plugins.validate(dir).await;
    if !problems.is_empty() {
        print_problems(&problems);
        bail!("the plugin has problems; fix them and run `pt plugin install` again");
    }
    let manifest = Manifest::load(dir).map_err(|p| anyhow::anyhow!("{p}"))?;
    let dir = dir.canonicalize()?;
    let stored = pathdiff(&dir, ws.root()).unwrap_or_else(|| dir.to_string_lossy().into_owned());

    let mut settings = ws.settings()?;
    if !settings.plugins.contains(&stored) {
        settings.plugins.push(stored.clone());
    }
    println!("{} {} {} — {}", style::green("✓"), style::bold(manifest.display_name()), manifest.version, manifest.description);
    let approved = ask_permissions(&manifest, yes)?;
    if approved {
        settings.approved_permissions.insert(manifest.name.clone(), manifest.permissions.clone());
    }
    ws.save_settings(&settings)?;
    println!("Installed from {stored}.");
    if !approved && !manifest.permissions.is_empty() {
        println!("Permissions are not approved yet. Approve later with `pt plugin approve {}`", manifest.name);
    }
    Ok(ExitCode::SUCCESS)
}

/// Shows each permission in plain words and asks. Returns true when approved.
fn ask_permissions(manifest: &Manifest, yes: bool) -> Result<bool> {
    if manifest.permissions.is_empty() {
        println!("  This plugin needs no permissions.");
        return Ok(true);
    }
    println!("  This plugin asks to:");
    for permission in manifest.parsed_permissions() {
        let warning = matches!(&permission, Permission::Network(host) if host == "*");
        let line = format!("    • {}", permission.explain());
        println!("{}", if warning { style::yellow(line) } else { line });
    }
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }
    print!("  Allow? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

/// `dir` relative to `base`, when `dir` is inside `base`.
fn pathdiff(dir: &Path, base: &Path) -> Option<String> {
    let base = base.canonicalize().ok()?;
    dir.strip_prefix(&base).ok().map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// Finds the settings entry and manifest of an installed plugin by name.
fn find_installed(ws: &post_tester_core::Workspace, name: &str) -> Result<(String, Manifest)> {
    for entry in ws.settings()?.plugins {
        if let Ok(manifest) = Manifest::load(&ws.root().join(&entry))
            && manifest.name == name
        {
            return Ok((entry, manifest));
        }
    }
    bail!("plugin `{name}` is not installed in this workspace. See `pt plugin list`")
}

async fn approve(workspace: Option<&Path>, name: &str) -> Result<ExitCode> {
    let ctx = Context::open(workspace, true).await?;
    let ws = ctx.workspace()?;
    let (_, manifest) = find_installed(&ws, name)?;
    if !ask_permissions(&manifest, !std::io::stdin().is_terminal())? {
        println!("Not approved.");
        return Ok(ExitCode::from(1));
    }
    let mut settings = ws.settings()?;
    settings.approved_permissions.insert(manifest.name.clone(), manifest.permissions.clone());
    ws.save_settings(&settings)?;
    println!("{} Approved {} permission(s) for {name}", style::green("✓"), manifest.permissions.len());
    Ok(ExitCode::SUCCESS)
}

async fn remove(workspace: Option<&Path>, name: &str) -> Result<ExitCode> {
    let ctx = Context::open(workspace, true).await?;
    let ws = ctx.workspace()?;
    let (entry, _) = find_installed(&ws, name)?;
    let mut settings = ws.settings()?;
    settings.plugins.retain(|p| p != &entry);
    settings.approved_permissions.remove(name);
    ws.save_settings(&settings).context("cannot save post-tester.yaml")?;
    println!("{} Removed {name} (files in {entry} were kept)", style::green("✓"));
    Ok(ExitCode::SUCCESS)
}

async fn list(workspace: Option<&Path>) -> Result<ExitCode> {
    let ctx = Context::open(workspace, true).await?;
    let plugins = ctx.plugins.plugins();
    if plugins.is_empty() {
        println!("No plugins installed. Create one with `pt plugin new <type> <name>`.");
        println!("Types: {}", PLUGIN_TYPES.iter().map(|(t, _)| *t).collect::<Vec<_>>().join(", "));
    }
    for plugin in plugins {
        let info = plugin.info();
        println!("{} {}  {}", style::bold(&info.name), info.version, style::dimmed(info.provides.join(", ")));
        println!("  {}", info.description);
        if !info.pending_permissions.is_empty() {
            println!("  {} {}", style::yellow("waiting for approval:"), info.pending_permissions.join(", "));
        }
    }
    Ok(ExitCode::SUCCESS)
}
