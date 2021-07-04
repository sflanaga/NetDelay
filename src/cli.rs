use std::sync::{Arc,Mutex,Condvar};
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use structopt::StructOpt;
use lazy_static::lazy_static;
use anyhow::{anyhow, Context};
use std::net::{TcpListener, TcpStream, SocketAddr, IpAddr};
use std::time::{Instant, Duration};
use log::LevelFilter;
use humantime::parse_duration;
use crate::util::str_to_socketaddr;

use crate::util::{to_log_level, to_duration, to_size_usize};
use std::path::PathBuf;

type Result<T> = anyhow::Result<T, anyhow::Error>;

lazy_static! {
    pub static ref BUILD_INFO: String  = format!("  ver: {}  rev: {}",
        env!("CARGO_PKG_VERSION"), env!("BUILD_GIT_HASH"));
}

#[derive(StructOpt, Debug, Clone)]
#[structopt(
version = BUILD_INFO.as_str(), rename_all = "kebab-case",
global_settings(& [structopt::clap::AppSettings::ColoredHelp, structopt::clap::AppSettings::DeriveDisplayOrder]),
)]
pub struct Cli {
    #[structopt(short, long, conflicts_with("client"))]
    /// server mode - note ip:port binding address is optional
    pub server: Option<Option<String>>,

    #[structopt(short, long, conflicts_with("server"), parse(try_from_str = str_to_socketaddr))]
    /// client ip:port of server end to connect too
    pub client: Option<SocketAddr>,

    #[structopt(short, long, default_value("15s"), parse(try_from_str = dur_from_str))]
    /// timeout for tcp socket
    pub timeout_socket: Duration,

    #[structopt(short, long, default_value("5150"))]
    /// port default to 5150 but this overrides that
    pub port: u16,

    #[structopt(short="T", long, parse(try_from_str = dur_from_str))]
    /// ticker interval
    ///
    /// examples: 5 = 5 seconds,  1s = 1 second, 100ms500us
    pub ticker_interval: Option<Duration>,

    #[structopt(short, long, parse(try_from_str = dur_from_str))]
    /// how to wait between tcp pings
    ///
    /// examples: 5 = 5 seconds,  1s = 1 second, 100ms500us
    pub interval: Option<Duration>,

    #[structopt(long, default_value("1s"), parse(try_from_str = dur_from_str))]
    /// info level logging threshold for turn-around time
    pub info_threshold: Duration,

    #[structopt(long, default_value("15s"), parse(try_from_str = dur_from_str))]
    /// warn level logging threshold for turn-around time
    pub warn_threshold: Duration,

    #[structopt(short = "L", long, parse(try_from_str = to_log_level), default_value("info"), conflicts_with("log_config"))]
    /// log level
    pub log_level: LevelFilter,

    #[structopt(short = "C", long)]
    /// use log4rs configuration file
    pub log_config: Option<PathBuf>
}


pub fn dur_from_str(s: &str) -> Result<Duration> {
    let mut num = String::new();
    let mut unit = String::new();
    let mut tot_secs = 0u64;
    let mut list = vec![];
    let mut have_unit = false;
    for c in s.chars() {
        if c >= '0' && c <= '9' {
            if have_unit {
                list.push((num.clone(), unit.clone()));
                num.clear();
                unit.clear();
            }
            num.push(c);
            have_unit = false;
        } else {
            unit.push(c);
            have_unit = true;
        }
    }
    list.push((num.clone(), unit.clone()));

    let mut tot = 0;
    for t in list.iter() {
        let v = t.0.parse::<u64>()?;
        let multi: u64 = match t.1.as_str() {
            "ns" => 1,
            "us" => 1000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 1_000_000_000 * 60,
            _ => Err(anyhow!("time unit \"{}\" not supported", &t.1))?
        };
        tot += v * multi;
    }
    Ok(Duration::from_nanos(tot))
}
