# zonecfg-backup

A small utility to backup zone configurations for all configured zones on an
illumos system.

This tool will essentially loop over all configured zones,
grab their zone config via `zonecfg -z ZONE info` and append the output to a
zstd compressed tar file.

## Configuration

[config.toml](config.toml) is provided as an example:
```toml
# directory to store/prune backups
outdir = "/backup/zones"

# number of zone backups to keep
number_of_backups = 5

# prefix used in file name. Ex: zcfgbak_1662780557.zones.tar.zst
prefix = "zcfgbak"

# zstd compression level 1-21
compression_level = 10
````

## Example
```
# zonecfg-backup config.toml
Sep 10 04:04:46.079 INFO appending zone github-sync
Sep 10 04:04:46.094 INFO appending zone keeper
Sep 10 04:04:46.128 INFO appending zone homeapi
Sep 10 04:04:46.143 INFO appending zone dns
Sep 10 04:04:46.153 INFO appending zone irc
Sep 10 04:04:46.268 INFO zone backup file written to "/backup/zones/zcfgbak_1662782685.zones.tar.zst"
Sep 10 04:04:46.284 INFO pruned "zcfgbak_1662781648.zones.tar.zst"
```

```
# gtar --use-compress-program=zstd -tvf zcfgbak_1662782685.zones.tar.zst
---------- 0/0             893 1970-01-01 00:00 github-sync.zone
---------- 0/0            1124 1970-01-01 00:00 keeper.zone
---------- 0/0             960 1970-01-01 00:00 homeapi.zone
---------- 0/0             782 1970-01-01 00:00 dns.zone
---------- 0/0             784 1970-01-01 00:00 irc.zone
```
