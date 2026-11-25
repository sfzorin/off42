//! Independent merge QA: output path followed by input PDFs in their requested order.
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicUsize},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let output = PathBuf::from(args.next().ok_or("output PDF required")?);
    let inputs = args.map(PathBuf::from).collect::<Vec<_>>();
    let bytes =
        pdf42_core::merge::merge_files(&inputs, &AtomicUsize::new(0), &AtomicBool::new(false))?;
    suite42_core::write_new(&output, &bytes)?;
    println!("{}", output.display());
    Ok(())
}
