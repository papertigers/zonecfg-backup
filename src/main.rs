use std::{fs::File, io::BufRead, process::Command};

use anyhow::{bail, Context};
use slog::{o, warn, Drain, Logger};
use tar::Header;

const ZONEADM: &str = "/usr/sbin/zoneadm";
const ZONECFG: &str = "/usr/sbin/zonecfg";

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

    Ok(zoneadm
        .stdout
        .lines()
        .collect::<Result<Vec<String>, _>>()
        .context("failed to parse zoneadm output")?)
}

fn tar_zone_info(log: Logger) -> Result<(), anyhow::Error> {
    let zones = find_zones()?;
    let encoder = {
        let file = File::create("zones.tar.zst")?;
        zstd::Encoder::new(file, 0)?
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
            }
            // perhaps the zone no longer exists, let's log an error and move on
            Err(e) => warn!(&log, "no info for {zone}: {e:?}"),
        }
    }

    a.finish()?;

    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    let log = create_logger();
    tar_zone_info(log)?;

    Ok(())
}
