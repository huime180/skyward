//! Says which root manager's package a file is, taking the package out of an
//! archive of them first, as the manager page does:
//! `cargo run -p skyward-root --example kind -- <file>`.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: kind <package or archive>");
        std::process::exit(2);
    }
    let at = std::path::Path::new(&args[1]);
    let apk = match skyward_root::apk::expand(at, std::path::Path::new("/tmp/incoming")) {
        Ok(apk) => apk,
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1)
        }
    };
    match skyward_root::apk::kind(&apk) {
        Ok(kind) => {
            println!("{} is a {} package, {} bytes", apk.display(), kind.name(), std::fs::metadata(&apk).map(|m| m.len()).unwrap_or(0));
        }
        Err(e) => {
            eprintln!("{}: {e:#}", apk.display());
            std::process::exit(1)
        }
    }
}
