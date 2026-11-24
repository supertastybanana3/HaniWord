#[path = "../src/engine/windows_text.rs"]
mod windows_text;

use std::{fs, path::PathBuf, time::Instant};
use serde_json::{json, Value};

fn summary(samples: &[f64]) -> Value {
    let mut values = samples.to_vec();
    values.sort_by(f64::total_cmp);
    let at = |percent: f64| values[((values.len() - 1) as f64 * percent).ceil() as usize];
    json!({"samples": values.len(), "median_ms": at(0.5), "p95_ms": at(0.95),
        "max_ms": values.last(), "over_16_ms": values.iter().filter(|&&v| v > 16.0).count(),
        "over_50_ms": values.iter().filter(|&&v| v > 50.0).count(),
        "over_100_ms": values.iter().filter(|&&v| v > 100.0).count()})
}

fn main() -> anyhow::Result<()> {
    let out = PathBuf::from(std::env::args_os().nth(1).ok_or_else(|| anyhow::anyhow!("Output path required"))?);
    let root = out.parent().ok_or_else(|| anyhow::anyhow!("Output directory required"))?;
    anyhow::ensure!(root.join("isolated_latency_fixture.json").is_file(), "Isolated fixture marker required");
    let appdata = PathBuf::from(std::env::var_os("APPDATA").ok_or_else(|| anyhow::anyhow!("APPDATA required"))?);
    anyhow::ensure!(appdata.canonicalize()? == root.join("appdata").canonicalize()?, "Synthetic APPDATA required");
    anyhow::ensure!(!out.exists(), "Refusing to overwrite a report");
    let mut rows = Vec::new();
    let mut conversion_times = Vec::new();
    let mut prediction_times = Vec::new();
    let mut pair_times = Vec::new();
    let readings = [
        "\u{304d}\u{3087}\u{3046}\u{306f}\u{304c}\u{3063}\u{3053}\u{3046}\u{3067}\u{304b}\u{304c}\u{304f}\u{306e}\u{3058}\u{3063}\u{3051}\u{3093}\u{3092}\u{3057}\u{305f}",
        "\u{304e}\u{3083}\u{304f}\u{3086}\u{306b}\u{3085}\u{3046}\u{3054}\u{304c}",
        "\u{305b}\u{304d}\u{3085}\u{308a}\u{3066}\u{3043}\u{3081}\u{3093}\u{3067}",
    ];
    for round in 0..3 {
        for reading in readings {
            let chars: Vec<char> = reading.chars().collect();
            for length in 2..=chars.len() {
                let prefix: String = chars[..length].iter().collect();
                let start = Instant::now();
                let conversions = windows_text::conversions(&prefix);
                let conversion_ms = start.elapsed().as_secs_f64() * 1000.0;
                let start = Instant::now();
                let predictions = windows_text::predictions(&prefix);
                let prediction_ms = start.elapsed().as_secs_f64() * 1000.0;
                let cold = rows.is_empty();
                rows.push(json!({"round": round, "reading": prefix, "cold": cold,
                    "conversion_ms": conversion_ms, "prediction_ms": prediction_ms,
                    "combined_ms": conversion_ms + prediction_ms,
                    "conversions": conversions.len(), "predictions": predictions.len()}));
                if !cold {
                    conversion_times.push(conversion_ms);
                    prediction_times.push(prediction_ms);
                    pair_times.push(conversion_ms + prediction_ms);
                }
            }
        }
    }
    let report = json!({"status": "measured", "synthetic_input": true,
        "production_pipe_used": false, "user_keystrokes_sent": false,
        "conversion": summary(&conversion_times), "prediction": summary(&prediction_times),
        "combined": summary(&pair_times), "rows": rows});
    fs::write(out, serde_json::to_vec_pretty(&report)?)?;
    println!("Windows API timings: {}", report["combined"]);
    Ok(())
}
