use anyhow::{bail, Context};
use chrono::Utc;
use config::Config;
use slog::{info, o, warn, Drain, Logger};
use std::{
    cmp::Reverse,
    fs::{self, read_dir, File},
    io::BufRead,
    path::PathBuf,
    process::Command,
};
use tar::Header;

mod config;

const ZONEADM: &str = "/usr/sbin/zoneadm";
const ZONECFG: &str = "/usr/sbin/zonecfg";
const DEFAULT_PREFIX: &str = "zonecfg-backup";
const DEFAULT_COMPRESSION_LEVEL: i32 = 10;

fn create_logger() -> Logger {
    let plain = slog_term::PlainSyncDecorator::new(std::io::stdout());
    Logger::root(slog_term::FullFormat::new(plain).build().fuse(), o!())
}

fn get_zonecfg(zone: &str) -> Result<Vec<u8>, anyhow::Error> {
    let zonecfg = Command::new(ZONECFG)
        .env_clear()
        .args(["-z", zone, "info"])
        .output()?;

    if !zonecfg.status.success() {
        let stderr = String::from_utf8_lossy(&zonecfg.stderr);
        bail!("exec {ZONECFG}: failed {:?} -- {stderr}", zonecfg.status);
    }

    if zonecfg.stdout.is_empty() {
        bail!("no zonecfg info for {zone}?");
    }

    Ok(zonecfg.stdout)
}

fn find_zones() -> Result<Vec<String>, anyhow::Error> {
    let zoneadm = Command::new(ZONEADM)
        .env_clear()
        .args(["list", "-n", "-c"])
        .output()?;

    if !zoneadm.status.success() {
        bail!("exec {ZONEADM}: failed {:?}", zoneadm.status);
    }

    zoneadm
        .stdout
        .lines()
        .collect::<Result<Vec<String>, _>>()
        .context("failed to parse zoneadm output")
}

fn backup_zone_configs(c: &Ctx) -> Result<(), anyhow::Error> {
    let zones = find_zones()?;
    let prefix = c.config.prefix.as_deref().unwrap_or(DEFAULT_PREFIX);
    let now = Utc::now().timestamp();
    let path = PathBuf::from(&c.config.outdir).join(format!("{prefix}_{now}.zones.tar.zst"));
    let encoder = {
        let file = File::create(&path).with_context(|| format!("creating {path:?}"))?;
        let level = c
            .config
            .compression_level
            .unwrap_or(DEFAULT_COMPRESSION_LEVEL);
        zstd::Encoder::new(file, level)?
    }
    .auto_finish();
    let mut a = tar::Builder::new(encoder);

    for zone in zones {
        match get_zonecfg(&zone) {
            Ok(info) => {
                let mut header = Header::new_gnu();
                header.set_size(info.len() as u64);
                header.set_cksum();
                a.append_data(&mut header, format!("{zone}.zone"), info.as_slice())?;
                info!(&c.log, "appending zone {zone}");
            }
            // perhaps the zone no longer exists, let's log an error and move on
            Err(e) => warn!(c.log, "no info for {zone}: {e:?}"),
        }
    }

    a.finish()?;
    info!(&c.log, "zone backup file written to {path:?}");

    Ok(())
}

fn prune_zonecfg_backups(c: &Ctx) -> Result<(), anyhow::Error> {
    // find all entries in the directory, stopping on first error
    let mut ents = read_dir(&c.config.outdir)
        .with_context(|| format!("reading {:?}", c.config.outdir))?
        .collect::<Result<Vec<_>, _>>()?;
    // keep entries that start with our prefix
    ents.retain(|e| {
        e.file_name()
            .as_os_str()
            .to_str()
            .map(|f| f.starts_with(&c.config.prefix.as_deref().unwrap_or(DEFAULT_PREFIX)))
            .unwrap_or(false)
    });
    // sort them so that we delete the oldest backups
    ents.sort_by_key(|e| Reverse(e.file_name()));

    if c.config.number_of_backups < ents.len() {
        for to_remove in &ents[c.config.number_of_backups..] {
            let f = to_remove.file_name();
            fs::remove_file(&f).with_context(|| format!("removing file {f:?}"))?;
            info!(&c.log, "pruned {f:?}")
        }
    }

    Ok(())
}

struct Ctx {
    config: Config,
    log: Logger,
}

fn main() -> Result<(), anyhow::Error> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        bail!("usage: {} config_file", args[0]);
    }
    let ctx = Ctx {
        config: Config::from_file(&args[1])?,
        log: create_logger(),
    };

    if let Some(level) = ctx.config.compression_level {
        if !(1..=21).contains(&level) {
            bail!("compression level must be between 1-21");
        }
    }

    backup_zone_configs(&ctx)?;
    prune_zonecfg_backups(&ctx)?;

    Ok(())
}
