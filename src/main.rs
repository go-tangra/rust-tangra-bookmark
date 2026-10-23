//! `bookmarksvc`: the bookmark module (deployer `cmd/deployersvc`).
//!
//! ```text
//! bookmarksvc -config <path>                 migrate, then serve until SIGINT/SIGTERM
//! bookmarksvc bootstrap -config <path>       apply the migrations (db.migrate_dsn) and exit
//! bookmarksvc --healthcheck -config <path>   exit 0 when /readyz answers 200
//! bookmarksvc version                        print the release
//! ```

use std::process::ExitCode;

use tangra_bookmark::{VERSION, config, healthcheck, module};
use tangra_sdk::gateway;

const DEFAULT_CONFIG: &str = "/app/config.yaml";

/// Go `flag` style: `-config path`, `--config path`, `-config=path`.
fn config_path(args: &[String]) -> Result<String, String> {
    let mut it = args.iter();
    let mut path = DEFAULT_CONFIG.to_owned();
    while let Some(a) = it.next() {
        let flag = a.trim_start_matches('-');
        if !a.starts_with('-') {
            return Err(format!("unexpected argument {a:?}"));
        }
        match flag.split_once('=') {
            Some(("config", v)) => path = v.to_owned(),
            None if flag == "config" => {
                path = it.next().ok_or("flag needs an argument: -config")?.clone();
            }
            None if flag == "healthcheck" => {}
            _ => return Err(format!("flag provided but not defined: {a}")),
        }
    }
    Ok(path)
}

fn load(args: &[String]) -> Result<config::BookmarkFile, String> {
    let cfg = config::load(&config_path(args)?).map_err(|e| e.0)?;
    for w in cfg.module.warnings() {
        eprintln!("bookmarksvc: warning: {w}");
    }
    Ok(cfg)
}

async fn serve(args: &[String]) -> Result<(), String> {
    let cfg = load(args)?;
    let b = module::build(cfg, module::Options::default())
        .await
        .map_err(|e| e.0)?;
    b.run().await.map_err(|e| e.0)
}

/// Applies the embedded migrations with `db.migrate_dsn` (else `db.dsn`) and
/// exits (deployer `bootstrap`).
async fn bootstrap(args: &[String]) -> Result<(), String> {
    let cfg = load(args)?;
    module::migrate(&cfg).await.map_err(|e| e.0)
}

async fn health(args: &[String]) -> Result<(), String> {
    let cfg =
        tangra_sdk::config::Config::<config::BookmarkConfig>::load(config_path(args)?).map_err(|e| e.0)?;
    healthcheck::probe(&cfg.framework.admin.addr).await
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Report the release to the gateway at registration (Go:
    // gatewayclient.SetBuildVersion(version)).
    gateway::set_build_version(VERSION);
    let first = args.first().map(String::as_str);
    let res = match first {
        Some("version" | "-version" | "--version") => {
            println!("{VERSION}");
            Ok(())
        }
        Some("bootstrap") => bootstrap(&args[1..]).await,
        _ if args.iter().any(|a| a == "--healthcheck" || a == "-healthcheck") => health(&args).await,
        _ => serve(&args).await,
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bookmarksvc: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::config_path;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn flags() {
        assert_eq!(config_path(&args(&[])).unwrap(), "/app/config.yaml");
        assert_eq!(config_path(&args(&["-config", "a.yaml"])).unwrap(), "a.yaml");
        assert_eq!(config_path(&args(&["--config=b.yaml"])).unwrap(), "b.yaml");
        assert_eq!(
            config_path(&args(&["--healthcheck", "-config", "c"])).unwrap(),
            "c"
        );
        assert!(config_path(&args(&["-config"])).is_err());
        assert!(config_path(&args(&["-nope"])).is_err());
    }
}
