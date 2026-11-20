//! Reads real host metrics from /proc and pushes them into Home Assistant,
//! then aggregates the recorder SQLite database into CSV for R.

use rusqlite::Connection;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

const SENSORS: [&str; 3] = [
    "sensor.rust_load_1m",
    "sensor.rust_mem_used_pct",
    "sensor.rust_disk_used_pct",
];

fn read_load_1m() -> f64 {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

fn read_mem_used_pct() -> f64 {
    let Ok(text) = fs::read_to_string("/proc/meminfo") else {
        return 0.0;
    };
    let mut total = 0.0;
    let mut avail = 0.0;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("MemTotal:") => {
                total = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0)
            }
            Some("MemAvailable:") => {
                avail = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0)
            }
            _ => {}
        }
    }
    if total <= 0.0 {
        return 0.0;
    }
    (total - avail) / total * 100.0
}

fn read_disk_used_pct() -> f64 {
    let out = match std::process::Command::new("df").arg("-P").arg("/workspace").output() {
        Ok(out) => out,
        Err(_) => return 0.0,
    };
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().nth(4))
        .map(|p| p.trim_end_matches('%'))
        .and_then(|p| p.parse().ok())
        .unwrap_or(0.0)
}

fn post_state(base: &str, token: &str, entity: &str, value: f64) -> Result<(), String> {
    let payload = serde_json::json!({
        "state": format!("{value:.2}"),
        "attributes": {"friendly_name": entity}
    });
    let url = format!("{base}/api/states/{entity}");
    ureq::post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map(|_| ())
        .map_err(|e| format!("{entity}: {e}"))
}

fn feed(base: &str, token: &str, samples: u32) {
    for i in 0..samples {
        let values = [
            (SENSORS[0], read_load_1m()),
            (SENSORS[1], read_mem_used_pct()),
            (SENSORS[2], read_disk_used_pct()),
        ];
        for (entity, value) in values {
            if let Err(err) = post_state(base, token, entity, value) {
                eprintln!("push failed: {err}");
            }
        }
        println!(
            "sample {}/{} load={:.2} mem={:.1}% disk={:.0}%",
            i + 1,
            samples,
            values[0].1,
            values[1].1,
            values[2].1
        );
        if i + 1 < samples {
            sleep(Duration::from_secs(5));
        }
    }
}

fn aggregate(db_path: &str, out_path: &str, bucket_secs: i64) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Recorder stores each state change in `states` keyed by metadata_id;
    // `states_meta` maps those ids to entity ids.
    let mut stmt = conn
        .prepare(
            "SELECT m.entity_id, s.last_updated_ts, s.state
             FROM states s
             JOIN states_meta m ON m.metadata_id = s.metadata_id
             WHERE m.entity_id IN (?1, ?2, ?3)
             ORDER BY m.entity_id, s.last_updated_ts",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([SENSORS[0], SENSORS[1], SENSORS[2]], |row| {
            let entity: String = row.get(0)?;
            let ts: f64 = row.get(1)?;
            let state: String = row.get(2)?;
            Ok((entity, ts, state))
        })
        .map_err(|e| e.to_string())?;

    let mut buckets: BTreeMap<String, BTreeMap<i64, Vec<f64>>> = BTreeMap::new();
    let mut total_rows = 0usize;
    let mut skipped = 0usize;
    for row in rows {
        let (entity, ts, state) = row.map_err(|e| e.to_string())?;
        total_rows += 1;
        let Ok(value) = state.parse::<f64>() else {
            skipped += 1;
            continue;
        };
        buckets
            .entry(entity)
            .or_default()
            .entry(ts as i64 / bucket_secs)
            .or_default()
            .push(value);
    }

    let mut file = fs::File::create(out_path).map_err(|e| e.to_string())?;
    writeln!(file, "entity_id,hour_start_utc,n,min,max,mean").map_err(|e| e.to_string())?;
    let mut written = 0usize;
    for (entity, hours) in &buckets {
        for (hour, values) in hours {
            let sum: f64 = values.iter().sum();
            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            writeln!(
                file,
                "{entity},{},{},{min:.4},{max:.4},{:.4}",
                hour * bucket_secs,
                values.len(),
                sum / values.len() as f64
            )
            .map_err(|e| e.to_string())?;
            written += 1;
        }
    }

    eprintln!(
        "scanned {total_rows} state rows, skipped {skipped} non-numeric, wrote {written} buckets of {bucket_secs}s"
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str).unwrap_or("") {
        "feed" => {
            let base = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| "http://127.0.0.1:12000".into());
            let token = args.get(3).expect("usage: feed <base_url> <token> [samples]");
            let samples: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(12);
            feed(&base, token, samples);
        }
        "aggregate" => {
            let db = args.get(2).expect("usage: aggregate <db> <out_csv> [bucket_secs]");
            let out = args.get(3).expect("usage: aggregate <db> <out_csv> [bucket_secs]");
            let bucket_secs: i64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(3600);
            if let Err(err) = aggregate(db, out, bucket_secs) {
                eprintln!("aggregate failed: {err}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("usage: ha-rust-r <feed|aggregate> ...");
            std::process::exit(2);
        }
    }
}
