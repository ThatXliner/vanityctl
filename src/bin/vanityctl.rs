use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use reqwest::{Client, Method, Response};
use serde_json::Value;
use vanityctl::{ConfigPaths, HostConfig, adopt::LaunchdAdopter, runner::SystemRunner};

#[derive(Parser)]
#[command(
    name = "vanityctl",
    version,
    about = "One control plane for everything running on this computer",
    arg_required_else_help = true
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true, env = "VANITYCTL_API")]
    api: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Safely import an existing resource into vanityctl ownership
    Adopt {
        #[command(subcommand)]
        command: AdoptCommand,
    },
    List,
    #[command(alias = "ps")]
    Status {
        service: Option<String>,
    },
    Describe {
        service: String,
    },
    Start {
        service: String,
    },
    Stop {
        service: String,
    },
    Restart {
        service: String,
    },
    /// Pull images for a Compose service.
    Pull {
        service: String,
    },
    /// Build images for a Compose service.
    Build {
        service: String,
    },
    Logs {
        service: String,
        #[arg(short = 'f', long)]
        follow: bool,
        #[arg(long, default_value_t = 200)]
        lines: usize,
    },
    Deploy {
        target: String,
        name: Option<String>,
        #[arg(long)]
        retry: bool,
    },
    Apply {
        /// Validate and show resolved plugin/services without changing the machine.
        #[arg(long)]
        dry_run: bool,
    },
    /// Inspect resolved plugin instances and the bundled plugin library.
    Plugin {
        #[command(subcommand)]
        command: Option<PluginCommand>,
    },
    Run {
        service: String,
    },
    Jobs,
    History {
        service: String,
    },
    Enable {
        service: String,
    },
    Disable {
        service: String,
    },
    Doctor,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    AgentContext,
    Dns {
        #[command(subcommand)]
        command: Option<DnsCommand>,
    },
    Dashboard,
}

#[derive(Subcommand)]
enum AdoptCommand {
    /// Inspect and adopt an existing user LaunchAgent
    Launchd {
        label: String,
        #[arg(long = "as", value_name = "SERVICE")]
        service: String,
        /// Perform the handoff. Without this flag, only a redacted plan is shown.
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    Validate,
}
#[derive(Subcommand)]
enum DnsCommand {
    Status,
    Records,
    Reconcile,
}

#[derive(Subcommand)]
enum PluginCommand {
    List,
    Describe { instance: String },
    Library,
}

struct Api {
    base: String,
    token: Option<String>,
    client: Client,
}
impl Api {
    fn discover(override_url: Option<String>) -> Result<Self> {
        let config = HostConfig::load(&ConfigPaths::discover()?)?;
        let base = override_url.unwrap_or_else(|| format!("http://{}", config.api.listen));
        let token = config.api.resolve_token()?;
        Ok(Self {
            base: base.trim_end_matches('/').into(),
            token,
            client: Client::new(),
        })
    }
    async fn request(&self, method: Method, path: &str) -> Result<Response> {
        let mut request = self.client.request(method, format!("{}{path}", self.base));
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.with_context(|| {
            format!("cannot reach hostd at {}; start it with `hostd`", self.base)
        })?;
        if !response.status().is_success() {
            let status = response.status();
            let body: Value = response.json().await.unwrap_or_default();
            bail!(
                "hostd returned {status}: {}",
                body.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("request failed")
            );
        }
        Ok(response)
    }
    async fn json(&self, method: Method, path: &str) -> Result<Value> {
        Ok(self.request(method, path).await?.json().await?)
    }
    async fn text(&self, path: &str) -> Result<String> {
        Ok(self.request(Method::GET, path).await?.text().await?)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Command::Adopt {
        command:
            AdoptCommand::Launchd {
                label,
                service,
                execute,
            },
    } = &cli.command
    {
        let adopter =
            LaunchdAdopter::discover(ConfigPaths::discover()?, std::sync::Arc::new(SystemRunner))?;
        let result = adopter.adopt(label, service, *execute)?;
        if cli.json {
            print_json(&serde_json::to_value(result)?)?;
        } else {
            println!("{}", result.render());
        }
        return Ok(());
    }
    if matches!(cli.command, Command::Config { .. }) {
        let config = HostConfig::load(&ConfigPaths::discover()?)?;
        println!(
            "configuration valid: {} services, {} plugins (schema v{})",
            config.services.len(),
            config.resolved_plugins.len(),
            config.version
        );
        return Ok(());
    }
    let api = Api::discover(cli.api)?;
    match cli.command {
        Command::List => {
            let v = api.json(Method::GET, "/api/services").await?;
            if cli.json {
                print_json(&v)?
            } else {
                println!("NAME\tTYPE\tENABLED\tDESCRIPTION");
                for row in v.as_array().unwrap() {
                    println!(
                        "{}\t{}\t{}\t{}",
                        s(row, "name"),
                        s(row, "type"),
                        s(row, "enabled"),
                        s(row, "description")
                    );
                }
            }
        }
        Command::Status { service } => {
            let path = service
                .map(|n| format!("/api/services/{n}/status"))
                .unwrap_or("/api/status".into());
            let v = api.json(Method::GET, &path).await?;
            if cli.json {
                print_json(&v)?
            } else {
                print_status(&v);
            }
        }
        Command::Describe { service } => output(
            api.json(Method::GET, &format!("/api/services/{service}"))
                .await?,
            cli.json,
        )?,
        Command::Start { service } => action(&api, &service, "start").await?,
        Command::Stop { service } => action(&api, &service, "stop").await?,
        Command::Restart { service } => action(&api, &service, "restart").await?,
        Command::Pull { service } => action(&api, &service, "pull").await?,
        Command::Build { service } => action(&api, &service, "build").await?,
        Command::Logs {
            service,
            follow,
            lines,
        } => follow_logs(&api, &service, lines, follow).await?,
        Command::Deploy {
            target,
            name,
            retry,
        } => deploy_command(&api, &target, name.as_deref(), retry, cli.json).await?,
        Command::Apply { dry_run } => {
            let (method, path) = if dry_run {
                (Method::GET, "/api/apply/plan")
            } else {
                (Method::POST, "/api/apply")
            };
            output(api.json(method, path).await?, cli.json)?
        }
        Command::Plugin { command } => match command.unwrap_or(PluginCommand::List) {
            PluginCommand::List => output(api.json(Method::GET, "/api/plugins").await?, cli.json)?,
            PluginCommand::Describe { instance } => output(
                api.json(Method::GET, &format!("/api/plugins/{instance}"))
                    .await?,
                cli.json,
            )?,
            PluginCommand::Library => output(
                api.json(Method::GET, "/api/plugins/library").await?,
                cli.json,
            )?,
        },
        Command::Run { service } => output(
            api.json(Method::POST, &format!("/api/jobs/{service}/run"))
                .await?,
            cli.json,
        )?,
        Command::Jobs => {
            let v = api.json(Method::GET, "/api/jobs").await?;
            if cli.json {
                print_json(&v)?
            } else {
                print_status(&v);
            }
        }
        Command::History { service } => output(
            api.json(Method::GET, &format!("/api/jobs/{service}/history"))
                .await?,
            cli.json,
        )?,
        Command::Enable { service } => {
            api.json(Method::POST, &format!("/api/jobs/{service}/enable"))
                .await?;
            println!("enabled {service} (runtime override; apply restores YAML)");
        }
        Command::Disable { service } => {
            api.json(Method::POST, &format!("/api/jobs/{service}/disable"))
                .await?;
            println!("disabled {service} (runtime override; apply restores YAML)");
        }
        Command::Doctor => output(api.json(Method::GET, "/api/system").await?, cli.json)?,
        Command::AgentContext => print!("{}", api.text("/api/agent-context").await?),
        Command::Dns { command } => match command.unwrap_or(DnsCommand::Status) {
            DnsCommand::Status | DnsCommand::Records => {
                output(api.json(Method::GET, "/api/dns").await?, cli.json)?
            }
            DnsCommand::Reconcile => output(
                api.json(Method::POST, "/api/dns/reconcile").await?,
                cli.json,
            )?,
        },
        Command::Dashboard => println!("{}", api.base),
        Command::Adopt { .. } => unreachable!(),
        Command::Config { .. } => unreachable!(),
    }
    Ok(())
}

async fn action(api: &Api, name: &str, action: &str) -> Result<()> {
    api.json(Method::POST, &format!("/api/services/{name}/{action}"))
        .await?;
    println!("{action} requested for {name}");
    Ok(())
}
async fn deploy_command(
    api: &Api,
    target: &str,
    name: Option<&str>,
    retry: bool,
    json: bool,
) -> Result<()> {
    match target {
        "history" => {
            let name = name.context("usage: vanityctl deploy history <service>")?;
            output(
                api.json(Method::GET, &format!("/api/services/{name}/deployments"))
                    .await?,
                json,
            )
        }
        "logs" => {
            let name = name.context("usage: vanityctl deploy logs <service>")?;
            print!(
                "{}",
                api.text(&format!("/api/services/{name}/deployments/logs"))
                    .await?
            );
            Ok(())
        }
        "auto-enable" | "auto-disable" => {
            let name =
                name.context("usage: vanityctl deploy auto-enable|auto-disable <service>")?;
            api.json(
                Method::POST,
                &format!("/api/services/{name}/deploy/{target}"),
            )
            .await?;
            println!(
                "{} auto-deploy for {name} (runtime override; apply restores YAML)",
                if target == "auto-enable" {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            Ok(())
        }
        service => {
            if name.is_some() {
                bail!("unexpected argument; usage: vanityctl deploy <service>");
            }
            let q = if retry { "?retry=true" } else { "" };
            output(
                api.json(Method::POST, &format!("/api/services/{service}/deploy{q}"))
                    .await?,
                json,
            )
        }
    }
}
async fn follow_logs(api: &Api, name: &str, lines: usize, follow: bool) -> Result<()> {
    let path = format!("/api/services/{name}/logs?lines={lines}");
    let mut previous = api.text(&path).await?;
    println!("{previous}");
    if !follow {
        return Ok(());
    }
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let current = api.text(&path).await?;
        if let Some(new) = current.strip_prefix(&previous) {
            print!("{new}")
        } else if current != previous {
            println!("{current}")
        }
        previous = current;
    }
}
fn output(v: Value, json: bool) -> Result<()> {
    if json {
        print_json(&v)
    } else {
        println!("{}", serde_json::to_string_pretty(&v)?);
        Ok(())
    }
}
fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_else(|| {
        if v.get(key).and_then(Value::as_bool) == Some(true) {
            "true"
        } else if v.get(key).and_then(Value::as_bool) == Some(false) {
            "false"
        } else {
            "—"
        }
    })
}
fn print_status(v: &Value) {
    let rows: Vec<&Value> = match v.as_array() {
        Some(a) => a.iter().collect(),
        None => vec![v],
    };
    println!("NAME\tTYPE\tSTATE\tDEPLOY\tDETAILS");
    for r in rows {
        let deploy = r
            .get("deployment")
            .and_then(|d| d.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("—");
        let details = r.get("details").and_then(Value::as_str).unwrap_or("—");
        println!(
            "{}\t{}\t{}\t{}\t{}",
            s(r, "name"),
            s(r, "type"),
            s(r, "state"),
            deploy,
            details
        );
    }
}
