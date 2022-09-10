# zonecfg-backup

A small utility to backup zone configurations for all configured zones on an
illumos system.

This tool will essentially loop over all configured zones,
grab their zone config via `zonecfg -z ZONE info` and append the output to a
zstd compressed tar file.

Motivation behind this tool was to allow me to dump zone configs somewhere that
could be picked up by a tool like rsync/zrepl. Ideally I will one day create
zones via some other automation, but until then this saves me the hassle of
remembering to update the templates for my zones when I make one off changes
via something like zonecfg/zadm.

## Configuration

[config.toml](config.toml) is provided as an example:

| Option | Optional | Default | Explanation |
| ------ | -------- | ------- | ----------- |
| outdir | false | N/A | Directory to store/prune backups |
| number_of_backups | false | N/A | Number of zone backups to keep |
| prefix | true | `zonecfg-backup` | prefix used in file name `zcfgbak_1662780557.zones.tar.zst` |
| compression_level | true | 10 | zstd compression level (1-21) |

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
