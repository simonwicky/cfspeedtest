use cfspeedtest::speedtest;
use cfspeedtest::OutputFormat;
use cfspeedtest::SpeedTestCLIOptions;
use clap::Parser;
use std::net::IpAddr;
use std::time::Duration;

use speedtest::speed_test;

fn main() {
    env_logger::init();
    let options = SpeedTestCLIOptions::parse();
    if options.output_format == OutputFormat::StdOut {
        println!("Starting Cloudflare speed test");
    }

    let mut client_builder = reqwest::blocking::Client::builder();
    if options.ipv4 {
        client_builder = client_builder.local_address("0.0.0.0".parse::<IpAddr>().unwrap());
    } else if options.ipv6 {
        client_builder = client_builder.local_address("::1".parse::<IpAddr>().unwrap());
    }
    if let Some(timeout_secs) = options.timeout_secs {
        client_builder = client_builder.timeout(Some(Duration::from_secs(timeout_secs)));
    }

    let client = client_builder
        .build()
        .expect("Failed to initialize reqwest client");
    speed_test(client, options);
}
