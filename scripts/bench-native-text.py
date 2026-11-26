"""Compare the same public text measurement calls with the published alpha.1 implementation."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE = """
use std::{hint::black_box, time::Instant};
fn main() {
    let text = "item 0123 가나다 مرحبا ".repeat(16);
    let mut working = working::TextSystem::new();
    let mut published = published::TextSystem::new();
    let widths = [145.0, 280.0, 420.0, 711.0];
    for w in widths {
        assert_eq!(working.measure(&text, 16.0, 400, Some(w)), published.measure(&text, 16.0, 400, Some(w)));
    }
    let mut times = [Vec::new(),Vec::new()];
    for trial in 0..21 {
        for index in [trial%2,1-trial%2] {
            let start = Instant::now();
            for i in 0..2000 {
                let width = Some(widths[i % widths.len()]);
                let result = if index == 0 { working.measure(black_box(&text),16.0,400,width) }
                             else { published.measure(black_box(&text),16.0,400,width) };
                black_box(result);
            }
            times[index].push(start.elapsed().as_nanos()/2000);
        }
    }
    for (name, mut samples) in ["working","published"].into_iter().zip(times) {
        samples.sort_unstable();
        println!("{name}: {} ns/measurement ({}–{})", samples[10], samples[0], samples[20]);
    }
}
"""

with tempfile.TemporaryDirectory(prefix="serge-text-bench-") as scratch:
    app = Path(scratch)
    (app / "src").mkdir()
    (app / "src/main.rs").write_text(SOURCE)
    dep = json.dumps(str(ROOT / "crates/serge-native-text"))
    (app / "Cargo.toml").write_text(f'''[package]
name="serge-text-comparison"
version="0.0.0"
edition="2024"
[dependencies]
working={{package="serge-native-text",path={dep}}}
published={{package="serge-native-text",version="=0.1.0-alpha.1"}}
[workspace]
''')
    subprocess.run(["cargo", "run", "--release"], cwd=app, check=True,
                   env=dict(os.environ, CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(app / "target"))))
