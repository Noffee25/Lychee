//! Silent end-to-end check of the engine: `cargo run --example engine_check -- A B`
//! Plays A at volume 0, seeks, pre-loads B and reports the gapless hand-over.
use std::time::{Duration, Instant};
use lychee_audio::{Engine, EngineEvent};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let engine = Engine::new(None, 0.0);
    let events = engine.events();
    println!("devices: {:?}", Engine::device_names());
    engine.load(1, args[0].clone().into(), 0, true, 1.0);
    engine.preload(2, args[1].clone().into(), 1.0, false);
    let start = Instant::now();
    let mut seeked = false;
    while start.elapsed() < Duration::from_secs(6) {
        while let Ok(ev) = events.try_recv() {
            println!("{:>6} ms  event {:?}", start.elapsed().as_millis(), match &ev { EngineEvent::Loaded { token, duration_ms, info } => format!("Loaded {token} {duration_ms:?} {} {}Hz", info.codec, info.sample_rate), other => format!("{other:?}") });
        }
        if !seeked && start.elapsed() > Duration::from_millis(600) {
            engine.seek(2000); // jump near the end of the 3 s file to reach the hand-over quickly
            seeked = true;
        }
        std::thread::sleep(Duration::from_millis(250));
        println!("{:>6} ms  position {:?}", start.elapsed().as_millis(), engine.position());
    }
}
