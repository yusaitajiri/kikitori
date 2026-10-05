//! Renders a session folder's PDF with the app's Typst template, without the app:
//! `cargo run --example export_pdf -- <session folder> <out.pdf>`. Prints how long it took. With an
//! output ending in `.typ` it writes the Typst export instead, its screenshots copied beside it.

use std::path::Path;
use std::time::Instant;

use kikitori_lib::export::{ExportOptions, saving, typst_pdf};
use kikitori_lib::session::recovery;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(args.len() == 3, "usage: export_pdf <session folder> <out.pdf>");
    let folder = Path::new(&args[1]);
    let (session, _, _) = recovery::load(folder)?;
    if args[2].ends_with(".typ") {
        let out = Path::new(&args[2]);
        let links = saving::copy_images(&session, folder, out)?;
        std::fs::write(out, typst_pdf::typst_source(&session, &ExportOptions::default(), &links, "dev"))?;
        return Ok(());
    }
    let t = Instant::now();
    let pdf = typst_pdf::render(&session, &ExportOptions::default(), folder, "dev")?;
    std::fs::write(&args[2], &pdf)?;
    println!("{} bytes in {:?}", pdf.len(), t.elapsed());
    Ok(())
}
