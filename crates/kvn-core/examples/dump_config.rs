//! Dev tool: dump the generated Xray client config for a share link.
//!
//! ```text
//! cargo run -p kvn-core --example dump_config -- 'vless://...'
//! ```
//!
//! Prints the real config (credentials included) so it can be fed to
//! `xray -test -config`. This is a local development tool only.

use kvn_core::uri::parse_uri;
use kvn_core::xray::{build_client_config, BuildOptions};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(link) = args.get(1) else {
        eprintln!("usage: dump_config <vless://... | trojan://... | vmess://... | ss://...>");
        std::process::exit(2);
    };
    let profile = match parse_uri(link) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("parse error: {err}");
            std::process::exit(1);
        }
    };
    let opts = BuildOptions {
        api_port: 10085,
        error_log: Some("/tmp/kvn-dump-error.log".to_owned()),
        ..Default::default()
    };
    match build_client_config(&profile, &opts) {
        Ok(outcome) => {
            println!("{}", serde_json::to_string_pretty(&outcome.config).unwrap());
            for note in &outcome.notes {
                eprintln!("note: {note}");
            }
        }
        Err(err) => {
            eprintln!("config error: {err}");
            std::process::exit(1);
        }
    }
}
