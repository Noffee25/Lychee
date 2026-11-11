//! Print or edit a file's tags: `cargo run -p lychee-tagger --example tag -- FILE [field=value]...`
//! Field names are the editor's ids (`title`, `artist`, `track`, ...).

use lychee_tagger::{Field, file};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: tag FILE [field=value]...");
        std::process::exit(2);
    };
    let path = std::path::PathBuf::from(path);
    let read = file::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut after = read.tags.clone();
    for arg in args {
        let (id, value) = arg.split_once('=').expect("field=value");
        let field = Field::from_id(id).unwrap_or_else(|| panic!("unknown field {id}"));
        after.set(field, value);
    }
    if after != read.tags {
        file::write(&path, &read.tags, &after).expect("write");
    }
    let shown = file::read(&path).expect("read back");
    println!("{} · {} · {} ms", shown.info.format, shown.info.tag_format, shown.info.duration_ms);
    for field in Field::ALL {
        let v = shown.tags.get(field);
        if !v.is_empty() {
            println!("{:>26}: {v}", field.label());
        }
    }
    for p in &shown.tags.pictures {
        println!("{:>26}: {} {} bytes", p.kind.label(), p.mime, p.data.len());
    }
}
