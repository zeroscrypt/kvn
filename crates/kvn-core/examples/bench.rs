//! Reproducible micro-benchmarks for the hot paths of `kvn-core`:
//!
//! ```text
//! cargo run --release -p kvn-core --example bench
//! ```
//!
//! Prints nanoseconds per operation for: VLESS parse, Shadowsocks parse,
//! client-config build, stats-JSON parse and a full state-machine
//! connect→health→connected cycle. No external benchmark harness, no
//! invented numbers — run it yourself.

use std::hint::black_box;
use std::time::Instant;

use kvn_core::state::{Event, RetryPolicy, StateMachine};
use kvn_core::uri::parse_import_payload;
use kvn_core::xray::{build_client_config, BuildOptions};
use kvn_core::ProfileSource;

const VLESS_REALITY: &str = "vless://5783a3e7-e373-51cd-8642-c83782b807c5@vpn.example.com:443\
?type=xhttp&security=reality&pbk=BBBBpublicKeyExample000000000000000000000000000000\
&fp=chrome&sni=www.example.com&sid=0123456789abcdef&path=%2Fsplit&mode=auto\
&extra=%7B%22xPaddingBytes%22%3A%22100-1000%22%2C%22scMaxEachPostBytes%22%3A1000000%7D\
&flow=xtls-rprx-vision&encryption=none#bench-reality";

const VLESS_WS: &str = "vless://5783a3e7-e373-51cd-8642-c83782b807c5@vpn.example.com:443\
?type=ws&security=tls&sni=vpn.example.com&path=%2Fws&host=vpn.example.com&fp=firefox#bench-ws";

const SS_2022: &str = "ss://2022-blake3-aes-256-gcm:cGFzc3dvcmQtc2VjcmV0LXBhc3N3b3JkLXNlYw%3D%3D@vpn.example.com:443#bench-ss";

const PROFILE_COUNT: usize = 1000;
const BUILD_ITERATIONS: usize = 1000;
const SM_CYCLES: usize = 10_000;

fn bench<F: FnMut()>(name: &str, iterations: usize, mut f: F) {
    // Warm-up.
    for _ in 0..iterations / 10 + 1 {
        f();
    }
    let started = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = started.elapsed();
    let ns_per_op = elapsed.as_nanos() as f64 / iterations as f64;
    println!("{name:<34} {ns_per_op:>12.0} ns/op   ({iterations} iters)");
}

fn main() {
    println!("kvn-core micro-benchmarks (release)");
    println!("{:-<70}", "");

    bench("parse vless reality+xhttp", PROFILE_COUNT, || {
        let outcome = parse_import_payload(black_box(VLESS_REALITY), ProfileSource::UriImport);
        black_box(outcome.profiles.len());
    });

    bench("parse vless tls+ws", PROFILE_COUNT, || {
        let outcome = parse_import_payload(black_box(VLESS_WS), ProfileSource::UriImport);
        black_box(outcome.profiles.len());
    });

    bench("parse shadowsocks-2022", PROFILE_COUNT, || {
        let outcome = parse_import_payload(black_box(SS_2022), ProfileSource::UriImport);
        black_box(outcome.profiles.len());
    });

    let profile = parse_import_payload(VLESS_REALITY, ProfileSource::UriImport)
        .profiles
        .into_iter()
        .next()
        .expect("bench profile must parse");

    bench("build xray client config", BUILD_ITERATIONS, || {
        let outcome = build_client_config(black_box(&profile), &BuildOptions::default());
        black_box(outcome.map(|o| o.config.to_string().len()).ok());
    });

    let stats_json = r#"{"stat":[{"name":"inbound>>>socks-in>>>traffic>>>uplink","value":12345},
        {"name":"inbound>>>socks-in>>>traffic>>>downlink","value":"67890"},
        {"name":"inbound>>>api>>>traffic>>>uplink"},
        {"name":"outbound>>>proxy>>>traffic>>>downlink","value":4321}]}"#;
    bench("parse stats json (4 counters)", PROFILE_COUNT, || {
        let snapshot = kvn_engine_stats_shim(black_box(stats_json));
        black_box(snapshot);
    });

    bench("state machine connect→ready→ok", SM_CYCLES, || {
        let mut sm = StateMachine::new(RetryPolicy::default());
        sm.handle(Event::ConnectRequested {
            profile_id: "bench".into(),
        });
        sm.handle(Event::EngineReady);
        sm.handle(Event::HealthCheckPassed { latency_ms: 42 });
        black_box(sm.state().label());
    });

    println!("{:-<70}", "");
    println!("note: stats parsing is re-implemented via kvn-engine; this row");
    println!("uses an equivalent local parser to keep this example self-contained.");
}

/// Minimal equivalent of `kvn_engine::stats::parse_stats_json` for timing
/// purposes only (the real one is unit-tested in the engine crate).
fn kvn_engine_stats_shim(json: &str) -> (u64, u64) {
    let value: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let mut up = 0u64;
    let mut down = 0u64;
    if let Some(stats) = value.get("stat").and_then(|s| s.as_array()) {
        for entry in stats {
            let name = entry.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let amount = entry
                .get("value")
                .and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .or_else(|| v.as_u64())
                })
                .unwrap_or(0);
            if !name.starts_with("inbound>>>") || name.starts_with("inbound>>>api>>>") {
                continue;
            }
            if name.ends_with(">>>uplink") {
                up += amount;
            } else if name.ends_with(">>>downlink") {
                down += amount;
            }
        }
    }
    (up, down)
}
