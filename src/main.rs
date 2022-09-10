use anyhow::{bail, Context};
use chrono::Utc;
use config::Config;
use sha2::{
    digest::{generic_array::GenericArray, typenum::U32},
    Digest, Sha256,
};
use slog::{info, o, warn, Drain, Logger};
use std::{
    cmp::Reverse,
    fs::{self, read_dir, File},
    io::{self, BufRead, Read},
    path::{Path, PathBuf},
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

fn find_latest_snapshot(c: &Ctx) -> Result<Option<PathBuf>, anyhow::Error> {
    let prefix = c.config.prefix.as_deref().unwrap_or(DEFAULT_PREFIX);
    let latest = c.config.outdir.join(format!("{prefix}_latest"));
    if Path::exists(&latest) {
        return fs::read_link(&latest).map(Some).map_err(From::from);
    }

    Ok(None)
}

fn snapshot_zone_configs(c: &Ctx) -> Result<tempfile::NamedTempFile, anyhow::Error> {
    let zones = find_zones()?;
    let tempfile = tempfile::NamedTempFile::new_in(&c.config.outdir)?;
    let level = c
        .config
        .compression_level
        .unwrap_or(DEFAULT_COMPRESSION_LEVEL);
    let mut encoder = zstd::Encoder::new(tempfile, level)?;
    {
        let mut a = tar::Builder::new(&mut encoder);
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
    }

    encoder.finish().map_err(From::from)
}

fn generate_hash<R: Read>(mut input: R) -> Result<GenericArray<u8, U32>, anyhow::Error> {
    let mut hasher = Sha256::new();
    io::copy(&mut input, &mut hasher)?;
    Ok(hasher.finalize())
}

fn try_commit_zone_snapshot(
    c: &Ctx,
    snapshot: tempfile::NamedTempFile,
) -> Result<(), anyhow::Error> {
    let prefix = c.config.prefix.as_deref().unwrap_or(DEFAULT_PREFIX);
    let now = Utc::now().timestamp();
    let path = PathBuf::from(&c.config.outdir).join(format!("{prefix}_{now}.zones.tar.zst"));
    let latest_path = c.config.outdir.join(format!("{prefix}_latest"));

    if let Some(latest) = find_latest_snapshot(c)? {
        let latest_file = File::open(&latest).with_context(|| format!("{latest:?}"))?;
        let latest_hash = generate_hash(&latest_file)?;
        // FIXME: why does generate_hash(snapshot.as_file()) differ from opening the file?
        let snapshot_file = File::open(snapshot.path())?;
        let snapshot_hash = generate_hash(snapshot_file)?;

        // As I understand it, zstd is deterministic in is compression output under the following conditions:
        // - zstd version does not change
        // - compression level does not change
        // If either of these conditions change in practice, the tool will simply just write a new backup file to disk.
        if latest_hash[..] == snapshot_hash[..] {
            info!(
                &c.log,
                "No changes in zone configs detected, skipping write."
            );

            return Ok(());
        }
    }

    snapshot
        .persist(&path)
        .with_context(|| format!("{path:?}"))?;
    info!(&c.log, "zone backup file written to {path:?}");
    let _ = fs::remove_file(&latest_path);
    std::os::unix::fs::symlink(&path, &latest_path)
        .with_context(|| format!("symlink {path:?} -> {latest_path:?}"))?;
    info!(&c.log, "symlinked {path:?} to {latest_path:?}");

    Ok(())
}

fn prune_zonecfg_backups(c: &Ctx) -> Result<(), anyhow::Error> {
    let prefix = c.config.prefix.as_deref().unwrap_or(DEFAULT_PREFIX);
    let latest = c.config.outdir.join(format!("{prefix}_latest"));
    // find all entries in the directory, stopping on first error
    let mut ents = read_dir(&c.config.outdir)
        .with_context(|| format!("reading {:?}", c.config.outdir))?
        .collect::<Result<Vec<_>, _>>()?;
    // keep entries that start with our prefix
    let filter = |f: &str| -> bool { f.starts_with(&prefix) && f != latest.as_os_str() };
    ents.retain(|e| {
        e.file_name()
            .as_os_str()
            .to_str()
            .map(filter)
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

    let snapshot = snapshot_zone_configs(&ctx)?;
    try_commit_zone_snapshot(&ctx, snapshot)?;
    prune_zonecfg_backups(&ctx)?;

    Ok(())
}
