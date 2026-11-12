//! Takes one package out of an archive on its server, as the manager page
//! does: `cargo run -p skyward-net --example take -- <url> <word> <out>`.

use std::sync::{Arc, Mutex};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: take <url> <word> <out>");
        std::process::exit(2);
    }
    // The names are looked up by the system here, as a computer does.
    let servers: skyward_net::http::Servers = Arc::new(Mutex::new(Vec::new()));
    let agent = skyward_net::http::agent(servers);
    let out = std::path::Path::new(&args[3]);
    match skyward_net::range::take(&agent, &args[1], &args[2], out) {
        Ok(true) => println!("took {} bytes into {}", std::fs::metadata(out).map(|m| m.len()).unwrap_or(0), out.display()),
        Ok(false) => {
            println!("the package holds no file named for {}", args[2]);
            std::process::exit(1)
        }
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1)
        }
    }
}
