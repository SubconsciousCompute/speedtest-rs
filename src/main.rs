mod distance;
mod error;
mod speedtest;
mod speedtest_config;
mod speedtest_csv;
mod speedtest_servers_config;

use crate::speedtest_csv::SpeedTestCsvResult;
use chrono::Utc;
use clap::Parser;
use std::io::{self, Write};
use url::Url;

#[cfg(feature = "log")]
use tracing::debug;
#[cfg(feature = "log")]
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Don't run download test
    #[arg(long, default_value_t = false)]
    no_download: bool,

    /// Don't run upload test
    #[arg(long, default_value_t = false)]
    no_upload: bool,

    /// Display a list of speedtest.net servers sorted by distance
    #[arg(long, default_value_t = false)]
    list: bool,

    /// Generate and provide an URL to the speedtest.net share results image
    #[arg(long, default_value_t = false)]
    share: bool,

    /// Display values in bytes instead of bits.
    #[arg(long, default_value_t = false)]
    bytes: bool,

    /// Suppress verbose output, only show basic information
    #[arg(long, default_value_t = false)]
    simple: bool,

    /// Suppress verbose output, only show basic information in CSV format.
    /// Speeds listed in bit/s and not affected by --bytes.
    #[arg(long, default_value_t = false)]
    csv: bool,

    /// Print CSV headers
    #[arg(long, default_value_t = false)]
    csv_header: bool,

    /// Single character delimiter to use in CSV output
    #[arg(long, default_value_t = ',')]
    csv_delimiter: char,

    /// Address of speedtest-mini server
    #[arg(short, long)]
    mini: Option<String>,
}

fn main() -> Result<(), error::SpeedTestError> {
    #[cfg(feature = "log")]
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let matches = Cli::parse();

    // This appears to be purely informational.
    if matches.csv_header {
        #[allow(unused)]
        let results = speedtest_csv::SpeedTestCsvResult::default();

        #[cfg(feature = "log")]
        debug!("{}", results.header_serialize());
        return Ok(());
    }

    let machine_format = matches.csv;

    if !matches.simple && !machine_format {
        #[cfg(feature = "log")]
        debug!("Retrieving speedtest.net configuration...");
    }
    let mut config = speedtest::get_configuration()?;

    let mut server_list_sorted;
    if let Some(mini) = matches.mini {
        let mini_url = Url::parse(&mini).unwrap();

        // matches.value_of("mini").unwrap().to_string()

        let host = mini_url.host().unwrap().to_string();
        let hostport = mini_url //
            .port()
            .map_or_else(
                || format!("{}:{}", mini_url.host().unwrap(), mini_url.port().unwrap()),
                |_| host.to_string(),
            );

        let mut path = mini_url.path();
        if path == "/" {
            path = "/speedtest/upload.php";
        }

        let url = format!("{}://{hostport}{path}", mini_url.scheme());

        server_list_sorted = vec![speedtest::SpeedTestServer {
            country: host.to_string(),
            host: hostport,
            id: 0,
            location: distance::EarthLocation {
                latitude: 0.0,
                longitude: 0.0,
            },
            distance: None,
            name: host.to_string(),
            sponsor: host,
            url,
        }]
    } else {
        if !matches.simple && !machine_format {
            #[cfg(feature = "log")]
            debug!("Retrieving speedtest.net server list...");
        }
        let server_list = speedtest::get_server_list_with_config(&config)?;
        server_list_sorted = server_list.servers_sorted_by_distance(&config);

        if matches.list {
            #[allow(unused)]
            for server in server_list_sorted {
                #[cfg(feature = "log")]
                debug!(
                    "{:4}) {} ({}, {}) [{}]",
                    server.id,
                    server.sponsor,
                    server.name,
                    server.country,
                    server
                        .distance
                        .map_or_else(|| "None".to_string(), |d| format!("{d:.2} km")),
                );
            }
            return Ok(());
        }
        if !matches.simple && !machine_format {
            #[cfg(feature = "log")]
            debug!(
                "Testing from {} ({})...",
                config.client.isp, config.client.ip
            );
            #[cfg(feature = "log")]
            debug!("Selecting best server based on latency...");
        }

        #[cfg(feature = "log")]
        debug!("Five Closest Servers");
        server_list_sorted.truncate(5);
        for _server in &server_list_sorted {
            #[cfg(feature = "log")]
            debug!("Close Server: {_server:?}");
        }
    }
    let latency_test_result = speedtest::get_best_server_based_on_latency(&server_list_sorted[..])?;

    if !machine_format {
        if !matches.simple {
            #[cfg(feature = "log")]
            debug!(
                "Hosted by {} ({}){}: {}.{} ms",
                latency_test_result.server.sponsor,
                latency_test_result.server.name,
                latency_test_result
                    .server
                    .distance
                    .map_or("".to_string(), |d| format!(" [{d:.2} km]")),
                latency_test_result.latency.as_millis(),
                latency_test_result.latency.as_micros() % 1000,
            );
        } else {
            #[cfg(feature = "log")]
            debug!(
                "Ping: {}.{} ms",
                latency_test_result.latency.as_millis(),
                latency_test_result.latency.as_millis() % 1000,
            );
        }
    }

    let best_server = latency_test_result.server;

    let download_measurement;
    let inner_download_measurement;

    if !matches.no_download {
        if !matches.simple && !machine_format {
            print!("Testing download speed");
            inner_download_measurement = speedtest::test_download_with_progress_and_config(
                best_server,
                print_dot,
                &mut config,
            )?;
            #[cfg(feature = "log")]
            debug!();
        } else {
            inner_download_measurement =
                speedtest::test_download_with_progress_and_config(best_server, || {}, &mut config)?;
        }

        if !machine_format {
            if matches.bytes {
                #[cfg(feature = "log")]
                debug!(
                    "Download: {:.2} Mbyte/s",
                    ((inner_download_measurement.kbps() / 8) as f32 / 1000.00)
                );
            } else {
                #[cfg(feature = "log")]
                debug!(
                    "Download: {:.2} Mbit/s",
                    (inner_download_measurement.kbps()) as f32 / 1000.00
                );
            }
        }
        download_measurement = Some(&inner_download_measurement);
    } else {
        download_measurement = None;
    }

    let upload_measurement;
    let inner_upload_measurement;

    if !matches.no_upload {
        if !matches.simple && !machine_format {
            print!("Testing upload speed");
            inner_upload_measurement =
                speedtest::test_upload_with_progress_and_config(best_server, print_dot, &config)?;
            #[cfg(feature = "log")]
            debug!();
        } else {
            inner_upload_measurement =
                speedtest::test_upload_with_progress_and_config(best_server, || {}, &config)?;
        }

        if !machine_format {
            if matches.bytes {
                #[cfg(feature = "log")]
                debug!(
                    "Upload: {:.2} Mbyte/s",
                    ((inner_upload_measurement.kbps() / 8) as f32 / 1000.00)
                );
            } else {
                #[cfg(feature = "log")]
                debug!(
                    "Upload: {:.2} Mbit/s",
                    (inner_upload_measurement.kbps() as f32 / 1000.00)
                );
            }
        }
        upload_measurement = Some(&inner_upload_measurement);
    } else {
        upload_measurement = None;
    }

    let speedtest_result = speedtest::SpeedTestResult {
        download_measurement,
        upload_measurement,
        server: best_server,
        latency_measurement: &latency_test_result,
    };

    if matches.csv {
        let speedtest_csv_result = SpeedTestCsvResult {
            server_id: &best_server.id.to_string(),
            sponsor: &best_server.sponsor,
            server_name: &best_server.name,
            timestamp: &Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            distance: &(latency_test_result
                .server
                .distance
                .map_or("".to_string(), |d| format!("{d:.14}")))[..],
            ping: &format!(
                "{}.{}",
                latency_test_result.latency.as_millis(),
                latency_test_result.latency.as_micros() % 1000
            ),
            download: &download_measurement
                .map_or(0.0, |x| x.bps_f64())
                .to_string(),
            upload: &upload_measurement.map_or(0.0, |x| x.bps_f64()).to_string(),
            share: &if matches.share {
                speedtest::get_share_url(&speedtest_result)?
            } else {
                "".to_string()
            },
            ip_address: &config.client.ip.to_string(),
        };
        let mut wtr = csv::WriterBuilder::new()
            .has_headers(false)
            .delimiter(matches.csv_delimiter as u8)
            .from_writer(io::stdout());
        wtr.serialize(speedtest_csv_result)?;
        wtr.flush()?;
        return Ok(());
    }

    if matches.share && !machine_format {
        #[cfg(feature = "log")]
        debug!("Share Request {speedtest_result:?}",);
        #[cfg(feature = "log")]
        debug!(
            "Share results: {}",
            speedtest::get_share_url(&speedtest_result)?
        );
    }

    if let (Some(download_measurement), Some(upload_measurement)) =
        (download_measurement, upload_measurement)
    {
        if !machine_format
            && ((download_measurement.kbps() as f32 / 1000.00) > 200.0
                || (upload_measurement.kbps() as f32 / 1000.00) > 200.0)
        {
            #[cfg(feature = "log")]
            debug!("WARNING: This tool may not be accurate for high bandwidth connections! Consider using a socket-based client alternative.")
        }
    }
    Ok(())
}

fn print_dot() {
    print!(".");
    io::stdout().flush().unwrap();
}
