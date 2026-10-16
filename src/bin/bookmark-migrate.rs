//! `bookmark-migrate`: imports the v3 bookmark tables into the v4 schema
//! (sms-gw `cmd/smsgw-migrate`; [`tangra_bookmark::migrate3`]).
//!
//! ```text
//! bookmark-migrate -source-config import.yaml -config /app/config.yaml -dry-run
//! bookmark-migrate -source-config import.yaml -config /app/config.yaml -apply
//! ```
//!
//! The source is opened read-only (one repeatable-read snapshot); its DSN is
//! a secret reference in the source configuration, never a flag. The
//! destination is `db.migrate_dsn` (else `db.dsn`) of the service
//! configuration. The JSON report goes to stdout; the exit status is 0 when
//! the run succeeded (or was already applied), 1 when it failed, 2 on a
//! usage error.

use std::process::ExitCode;

use tangra_bookmark::config::BookmarkFile;
use tangra_bookmark::migrate3::{self, Mapping, Mode, Options, Overrides, SourceConfig, Status};
use tangra_sdk::db::{self, DbConfig};

const USAGE: &str = "usage: bookmark-migrate -source-config <file> -config <file> (-dry-run | -apply)
  -source-config <file>   import configuration (source DSN reference, tenants, users, roles)
  -config <file>          bookmark service configuration (destination db.migrate_dsn)
  -tenant <v3=uuid,...>   v3 tenant ID to v4 tenant UUID (tenant 0 defaults to the platform tenant)
  -users <file>           email,uuid CSV of the v4 users (with the source's sys_users)
  -v3-users <file>        v3id,uuid CSV (direct mapping; wins over e-mail)
  -roles <v3=v4,...>      v3 role code to v4 role slug
  -platform-actor <id>    author recorded for v3 rows without one
  -exclude-orphans        leave out (and list) rows with unmapped references instead of failing
  -exclude-invalid        leave out (and list) rows the v4 schema refuses instead of failing
  -dry-run | -apply       compare only | write in one transaction and reconcile";

#[derive(Debug, Default)]
struct Args {
    source_config: String,
    config: String,
    over: Overrides,
    dry_run: bool,
    apply: bool,
    exclude_orphans: bool,
    exclude_invalid: bool,
}

/// Go `flag` style: `-name value`, `--name value`, `-name=value`.
fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        config: "/app/config.yaml".into(),
        ..Args::default()
    };
    let mut it = args.iter();
    while let Some(raw) = it.next() {
        if !raw.starts_with('-') {
            return Err(format!("unexpected argument {raw:?}"));
        }
        let flag = raw.trim_start_matches('-');
        let (name, inline) = match flag.split_once('=') {
            Some((n, v)) => (n, Some(v.to_owned())),
            None => (flag, None),
        };
        let mut value = || -> Result<String, String> {
            match &inline {
                Some(v) => Ok(v.clone()),
                None => it
                    .next()
                    .cloned()
                    .ok_or(format!("flag needs an argument: -{name}")),
            }
        };
        match name {
            "source-config" => a.source_config = value()?,
            "config" => a.config = value()?,
            "tenant" => a.over.tenants = value()?,
            "users" => a.over.users = value()?,
            "v3-users" => a.over.v3_users = value()?,
            "roles" => a.over.roles = value()?,
            "platform-actor" => a.over.platform_actor = Some(value()?),
            "dry-run" if inline.is_none() => a.dry_run = true,
            "apply" if inline.is_none() => a.apply = true,
            "exclude-orphans" if inline.is_none() => a.exclude_orphans = true,
            "exclude-invalid" if inline.is_none() => a.exclude_invalid = true,
            _ => return Err(format!("flag provided but not defined: {raw}")),
        }
    }
    if a.dry_run == a.apply {
        return Err("exactly one of -dry-run or -apply is required".into());
    }
    if a.source_config.is_empty() {
        return Err("-source-config is required".into());
    }
    Ok(a)
}

async fn run(a: Args) -> Result<Status, String> {
    let src_cfg = SourceConfig::load(&a.source_config).map_err(|e| e.0)?;
    let m = Mapping::build(&src_cfg, &a.over).map_err(|e| e.0)?;
    let o = Options {
        exclude_orphans: a.exclude_orphans || src_cfg.exclude_orphans,
        exclude_invalid: a.exclude_invalid || src_cfg.exclude_invalid,
    };
    // The service file need not validate in full here (identity, issuer):
    // only its db section is used.
    let cfg = BookmarkFile::load(&a.config).map_err(|e| e.0)?;
    let dst = db::connect(&DbConfig {
        dsn: cfg.module.db.effective_migrate_dsn().to_owned(),
        migrate_dsn: String::new(),
        max_conns: 2,
    })
    .await
    .map_err(|e| format!("destination: {e}"))?;
    let dsn = src_cfg.source.dsn.read().map_err(|e| e.0)?;
    let mut src = migrate3::connect_source(&dsn).await.map_err(|e| e.0)?;
    let mode = if a.apply { Mode::Apply } else { Mode::DryRun };
    let rep = migrate3::import(&mut src, &dst, &m, mode, o)
        .await
        .map_err(|e| e.0)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&rep).map_err(|e| e.to_string())?
    );
    Ok(rep.status)
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "-h" | "-help" | "--help"))
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let a = match parse(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bookmark-migrate: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(a).await {
        Ok(Status::Failed) => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bookmark-migrate: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn flags() {
        let a = parse(&args(&[
            "-source-config",
            "s.yaml",
            "--config=c.yaml",
            "-tenant",
            "0=x",
            "-dry-run",
            "-exclude-orphans",
        ]))
        .unwrap();
        assert_eq!(
            (a.source_config.as_str(), a.config.as_str()),
            ("s.yaml", "c.yaml")
        );
        assert!(a.dry_run && a.exclude_orphans && !a.apply);
        assert_eq!(a.over.tenants, "0=x");
        assert!(parse(&args(&["-source-config", "s", "-dry-run", "-apply"])).is_err());
        assert!(parse(&args(&["-source-config", "s"])).is_err());
        assert!(parse(&args(&["-dry-run"])).is_err());
        assert!(parse(&args(&["-source-config", "s", "-apply", "-nope"])).is_err());
        assert!(parse(&args(&["-source-config", "s", "-apply=1"])).is_err());
        assert!(parse(&args(&["-source-config", "s", "-apply", "-roles"])).is_err());
    }
}
