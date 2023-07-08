use std::{
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub outdir: PathBuf,
    pub number_of_backups: usize,
    pub prefix: Option<String>,
    pub compression_level: Option<i32>,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, anyhow::Error> {
        let path = path.as_ref();

        let f = File::open(path).with_context(|| format!("{path:?}"))?;
        let mut br = BufReader::new(f);
        let mut buf: Vec<u8> = Vec::new();

        br.read_to_end(&mut buf)?;
        let config: Self = toml::from_slice(&buf).context("failed to parse config file")?;

        Ok(config)
    }
}
