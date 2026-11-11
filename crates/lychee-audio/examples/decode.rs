//! Decode files completely and report what was found: `cargo run --example decode -- FILE...`
use std::time::Instant;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = std::path::Path::new(&arg);
        let started = Instant::now();
        match lychee_audio::Source::open(path) {
            Ok(mut src) => {
                let mut buf = Vec::new();
                let mut total = 0usize;
                loop {
                    buf.clear();
                    match src.decode_next(&mut buf) {
                        Ok(true) => total += buf.len(),
                        Ok(false) => break,
                        Err(e) => { println!("{arg}: error {e}"); break; }
                    }
                }
                let frames = total / src.channels;
                println!(
                    "{:<28} {:<7} {:>6} Hz {} ch  decoded {:>6.3}s  (container says {:?} ms)  in {:?}",
                    path.file_name().unwrap().to_string_lossy(), src.info.codec, src.sample_rate, src.channels,
                    frames as f64 / src.sample_rate as f64, src.duration_ms, started.elapsed()
                );
            }
            Err(e) => println!("{arg}: {e}"),
        }
    }
}
