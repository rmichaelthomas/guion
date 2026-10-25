use std::path::Path;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let Some(root) = manifest.parent() else {
        eprintln!("xtask package must be inside the workspace root");
        std::process::exit(2);
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(xtask::runner::run(root, &args));
}
