//! Try a live lookup: `cargo run -p lychee-lookup --example lookup -- "Artist" "Album"`

use lychee_lookup::matching::{self, FileFacts, Guess};
use lychee_lookup::Client;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (artist, album) = (args.first().cloned().unwrap_or_default(), args.get(1).cloned().unwrap_or_default());
    let client = Client::new();
    let guess = Guess { album: album.clone(), artist: artist.clone(), release_id: String::new(), file_count: 0 };
    let files: Vec<FileFacts> = Vec::new();
    let candidates = matching::find_candidates(&client, &files, &guess, 2).expect("search");
    for c in candidates.iter().take(6) {
        let s = &c.summary;
        println!("{:>3}  {} — {} ({}, {}, {} tracks, {}) {}", c.score, s.title, s.artist, s.date, s.country, s.track_count, s.format, s.id);
    }
    let Some(best) = candidates.iter().find(|c| c.release.is_some()) else { return };
    let release = best.release.as_ref().unwrap();
    println!("\n{} — {} · year {} · genre {:?} · {} {}", release.title, release.artist, release.year(), release.genre, release.label, release.catalog_number);
    for (m, t) in release.tracks().take(4) {
        println!("  {}.{:02} {} [{}] {:?} ms isrc={}", m.position, t.position, t.title, t.artist, t.length_ms, t.isrc);
    }
    match client.front_cover(&release.id, &release.release_group_id, 500) {
        Ok(Some(bytes)) => println!("cover: {} bytes, starts {:02x?}", bytes.len(), &bytes[..4]),
        other => println!("cover: {other:?}"),
    }
    let (m, t) = release.tracks().next().unwrap();
    let _ = m;
    match client.lyrics(&t.artist, &t.title, &release.title, t.length_ms.map(|ms| ms / 1000)) {
        Ok(Some(l)) => println!("lyrics: synced={} {} chars: {:?}", l.synced, l.text.len(), l.text.lines().next()),
        other => println!("lyrics: {other:?}"),
    }
}
