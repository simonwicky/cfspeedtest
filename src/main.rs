use regex::Regex;
use reqwest::{
    blocking::{Client, RequestBuilder},
    header::HeaderValue,
    StatusCode,
};
use std::{
    fmt::Display,
    time::{Duration, Instant},
};
mod progress;
use progress::print_progress;

const BASE_URL: &str = "http://speed.cloudflare.com";
const DOWNLOAD_URL: &str = "__down?bytes=";
const UPLOAD_URL: &str = "__up";
const NR_TEST_RUNS: u32 = 10;
const PAYLOAD_SIZES: [usize; 3] = [100_000, 1_000_000, 10_000_000];
const NR_LATENCY_TESTS: u32 = 25;

#[derive(Clone, Copy, Debug)]
enum TestType {
    Download,
    Upload,
}

struct Measurement {
    test_type: TestType,
    payload_size: usize,
    mbit: f64,
}

impl Display for Measurement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}: \t{}\t-> {}",
            self.test_type,
            format_bytes(self.payload_size),
            self.mbit,
        )
    }
}

struct Metadata {
    city: String,
    country: String,
    ip: String,
    asn: String,
    colo: String,
}

impl Display for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "City: {}\nCountry: {}\nIp: {}\nAsn: {}\nColo: {}",
            self.city, self.country, self.ip, self.asn, self.colo
        )
    }
}

fn main() {
    println!("Starting Cloudflare speed test");
    let client = reqwest::blocking::Client::new();
    speed_test(client);
}

fn speed_test(client: Client) {
    let metadata = fetch_metadata(&client);
    println!("{}", metadata);
    run_latency_test(&client);
    let _down_measurements = run_tests(&client, test_download, TestType::Download);
    let _up_measurements = run_tests(&client, test_upload, TestType::Upload);
}

fn run_latency_test(client: &Client) -> (Vec<f64>, f64) {
    let mut measurements: Vec<f64> = Vec::new();
    for i in 0..=NR_LATENCY_TESTS {
        print_progress("latency test", i, NR_LATENCY_TESTS);
        let latency = test_latency(client);
        measurements.push(latency);
    }
    let avg_latency = measurements.iter().sum::<f64>() / measurements.len() as f64;
    println!(
        "\nAvg GET request latency {:.2} ms (RTT excluding server processing time)\n",
        avg_latency
    );
    (measurements, avg_latency)
}

fn test_latency(client: &Client) -> f64 {
    let url = &format!("{}/{}{}", BASE_URL, DOWNLOAD_URL, 0);
    let req_builder = client.get(url);

    let start = Instant::now();
    let response = req_builder.send().expect("failed to get response");
    let _status_code = response.status();
    let duration = start.elapsed().as_secs_f64() * 1_000.0;

    let re = Regex::new(r"cfRequestDuration;dur=([\d.]+)").unwrap();
    let cf_req_duration: f64 = re
        .captures(
            response
                .headers()
                .get("Server-Timing")
                .expect("No Server-Timing in response header")
                .to_str()
                .unwrap(),
        )
        .unwrap()
        .get(1)
        .unwrap()
        .as_str()
        .parse()
        .unwrap();
    let mut req_latency = duration - cf_req_duration;
    if req_latency < 0.0 {
        // TODO investigate negative latency values
        req_latency = 0.0
    }
    req_latency
}

fn run_tests(
    client: &Client,
    test_fn: fn(&Client, usize) -> f64,
    test_type: TestType,
) -> Vec<Measurement> {
    let mut measurements: Vec<Measurement> = Vec::new();
    for payload_size in PAYLOAD_SIZES {
        for i in 0..NR_TEST_RUNS {
            print_progress(
                &format!("{:?} {:<5}", test_type, format_bytes(payload_size)),
                i,
                NR_TEST_RUNS - 1,
            );
            let mbit = test_fn(client, payload_size);
            measurements.push(Measurement {
                test_type,
                payload_size,
                mbit,
            });
        }
        println!()
    }
    log_measurements(&measurements);
    measurements
}

fn log_measurements(measurements: &[Measurement]) {
    // TODO calculate this for each payload size
    let min = measurements
        .iter()
        .map(|m| m.mbit)
        .fold(f64::INFINITY, |a, b| a.min(b));
    let max = measurements
        .iter()
        .map(|m| m.mbit)
        .fold(f64::NEG_INFINITY, |a, b| a.max(b));
    let avg: f64 = measurements.iter().map(|m| m.mbit).sum::<f64>() / measurements.len() as f64;

    // TODO draw boxplot etc
    println!(
        "{:?}: min {:.2}, max {:.2}, avg {:.2}\n",
        measurements[0].test_type, min, max, avg
    );
}

fn test_upload(client: &Client, payload_size_bytes: usize) -> f64 {
    let url = &format!("{}/{}", BASE_URL, UPLOAD_URL);
    let payload: Vec<u8> = vec![1; payload_size_bytes];
    let req_builder = client.post(url).body(payload);
    let (status_code, mbits, duration) = timed_send(req_builder, payload_size_bytes);
    print!(
        "\tupload {:.2} mbit/s with {} in {}ms -> post: {}",
        mbits,
        format_bytes(payload_size_bytes),
        duration.as_millis(),
        status_code
    );
    mbits
}

fn test_download(client: &Client, payload_size_bytes: usize) -> f64 {
    let url = &format!("{}/{}{}", BASE_URL, DOWNLOAD_URL, payload_size_bytes);
    let req_builder = client.get(url);
    let (status_code, mbits, duration) = timed_send(req_builder, payload_size_bytes);
    print!(
        "\tdownload {:.2} mbit/s with {} in {}ms -> get: {}",
        mbits,
        format_bytes(payload_size_bytes),
        duration.as_millis(),
        status_code
    );
    mbits
}

fn format_bytes(bytes: usize) -> String {
    match bytes {
        1_000..=999_999 => format!("{}KB", bytes / 1_000),
        1_000_000..=999_999_999 => format!("{}MB", bytes / 1_000_000),
        _ => format!("{} bytes", bytes),
    }
}

fn timed_send(
    req_builder: RequestBuilder,
    payload_size_bytes: usize,
) -> (StatusCode, f64, Duration) {
    let start = Instant::now();
    let response = req_builder.send().expect("failed to get response");
    let status_code = response.status();
    let _res_bytes = response.bytes();
    let duration = start.elapsed();
    let mbits = (payload_size_bytes as f64 * 8.0 / 1_000_000.0) / duration.as_secs_f64();
    (status_code, mbits, duration)
}

fn fetch_metadata(client: &Client) -> Metadata {
    let url = &format!("{}/{}{}", BASE_URL, DOWNLOAD_URL, 0);
    let headers = client
        .get(url)
        .send()
        .expect("failed to get response")
        .headers()
        .to_owned();
    Metadata {
        city: extract_header_value(&headers, "cf-meta-city", "City N/A"),
        country: extract_header_value(&headers, "cf-meta-country", "Country N/A"),
        ip: extract_header_value(&headers, "cf-meta-ip", "IP N/A"),
        asn: extract_header_value(&headers, "cf-meta-asn", "ASN N/A"),
        colo: extract_header_value(&headers, "cf-meta-colo", "Colo N/A"),
    }
}

fn extract_header_value(
    headers: &reqwest::header::HeaderMap,
    header_name: &str,
    na_value: &str,
) -> String {
    headers
        .get(header_name)
        .unwrap_or(&HeaderValue::from_str(na_value).unwrap())
        .to_str()
        .unwrap()
        .to_owned()
}