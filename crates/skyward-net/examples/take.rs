//! Takes one package out of an archive on its server, as the manager page
//! does: `cargo run -p skyward-net --example take -- <url> <word> <out>`.

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

/// The nameservers this computer uses, as a phone's recovery is given its
/// own: the agent looks names up nowhere else.
fn servers() -> Vec<Ipv4Addr> {
    let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") else { return Vec::new() };
    text.lines()
        .filter_map(|line| line.strip_prefix("nameserver "))
        .filter_map(|at| at.trim().parse().ok())
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: take <url> <word> <out>");
        std::process::exit(2);
    }
    let servers: skyward_net::http::Servers = Arc::new(Mutex::new(servers()));
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
