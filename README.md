# ha-rust-r-analytics

Rust + R analytics pipeline over Home Assistant recorder history.

Home Assistant Core is ~1.7M lines of Python across 1,593 integrations and
cannot be rewritten in Rust or R. What *can* be done — and is done here — is
putting Rust and R **around** it: Rust owns the data path (reading `/proc`,
scanning the recorder SQLite database, aggregating), R owns the statistics.

```
/proc ──► Rust `feed` ──► HA REST API ──► recorder SQLite ──► Rust `aggregate` ──► CSV ──► R ──► summary + PNG
```

## Requirements

- Rust (built and tested on 1.100.0)
- R (built and tested on 4.5.0)
- A running Home Assistant instance with the `recorder` integration enabled

Enable the recorder in `configuration.yaml` so states reach SQLite promptly:

```yaml
recorder:
  db_url: sqlite:////config/home-assistant_v2.db
  purge_keep_days: 30
  commit_interval: 1
```

## Build

```bash
cargo build --release
```

Dependencies: `rusqlite` (bundled SQLite, no system libsqlite3 needed), `ureq`,
`serde_json`.

## Usage

### 1. Get an access token

```bash
python3 get_token.py http://127.0.0.1:8123 <username> <password>
```

### 2. Push host metrics into Home Assistant

```bash
TOKEN=$(python3 get_token.py)
./target/release/ha-rust-r feed http://127.0.0.1:8123 "$TOKEN" 90
```

Takes 90 samples 5 seconds apart. Each sample reads real machine state and
POSTs three entities:

| Entity | Source |
|---|---|
| `sensor.rust_load_1m` | `/proc/loadavg` |
| `sensor.rust_mem_used_pct` | `/proc/meminfo` (`MemTotal` vs `MemAvailable`) |
| `sensor.rust_disk_used_pct` | `df -P /workspace` |

### 3. Aggregate recorder history into CSV

```bash
./target/release/ha-rust-r aggregate /config/home-assistant_v2.db out.csv 120
```

The optional fourth argument is the bucket width in seconds (default `3600`).

Output columns: `entity_id,hour_start_utc,n,min,max,mean`.

### 4. Analyse in R

```bash
Rscript analyze.R out.csv reports/
```

Writes `summary.csv` (samples, mean, sd, min, max, coefficient of variation per
entity) and `sensor_history.png` (mean per bucket with min–max whiskers).

## Example output

Real run on a sandbox host, 123 samples in 15 two-minute buckets:

```
        entity samples   mean      sd   min   max  cv_pct
 disk_used_pct       1 10.000      NA 10.00 10.00      NA
       load_1m     122  1.357 0.70051  0.54  3.12 51.6070
  mem_used_pct     123 17.425 0.03469 17.19 17.78  0.1991
```

Load average started at 3.12 while the Rust release build was running and fell
to 0.54 afterwards — hence the 51.6% coefficient of variation. Memory stayed
flat between 17.2% and 17.8%.

See `sensor_history.png` for the plot.

## Notes

- `disk_used_pct` only changes when the filesystem does, so it typically yields
  a single sample per run.
- Recorder stores every state change in `states` keyed by `metadata_id`;
  `states_meta` maps those ids to entity ids. The aggregator joins the two.
- Home Assistant already ships Rust internally via `orjson`, which handles all
  API JSON serialization.

## License

MIT
