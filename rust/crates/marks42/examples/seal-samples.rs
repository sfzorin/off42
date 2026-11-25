//! Renders every stamp template with its example texts, as Kit42 `MarkSamples.write` does on macOS, so the two
//! sets can be compared side by side: `seal-NNN-<id>.png` (impression (i + 1) × 104 729), a second impression
//! `seal-NNN-<id>-b.png` for every 20th template, and `hires-seal-000.png` at the document output scale (2).
//!
//! Usage: `cargo run -p marks42 --example seal-samples --release -- <output folder> [template id filter]`

use marks42::seal::{render, templates, SealSpec};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().unwrap_or_else(|| "seal-samples".into()));
    let filter = args.next();
    std::fs::create_dir_all(&out).expect("output folder");
    let save = |img: Option<image::RgbaImage>, name: String| {
        let img = img.unwrap_or_else(|| panic!("render {name}"));
        img.save(out.join(format!("{name}.png"))).expect("write PNG");
    };
    let all = templates();
    if filter.is_none() {
        save(render(&SealSpec::example(&all[0]), 104_729, 2.0), "hires-seal-000".into());
    }
    for (i, t) in all.iter().enumerate() {
        if filter.as_deref().is_some_and(|f| !t.id.contains(f)) {
            continue;
        }
        let spec = SealSpec::example(t);
        save(render(&spec, (i as u64 + 1).wrapping_mul(104_729), 1.0), format!("seal-{i:03}-{}", t.id));
        if i % 20 == 0 {
            save(render(&spec, (i as u64 + 7).wrapping_mul(15_485_863), 1.0), format!("seal-{i:03}-{}-b", t.id));
        }
    }
    println!("{} templates written to {}", all.len(), out.display());
}
