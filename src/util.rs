use std::io::Write;

use chrono::Utc;
//use env_logger; //::{Builder, Env, fmt::{Color, Formatter}};
use log::LevelFilter;
use std::time::{Duration, Instant, SystemTime};
use anyhow::{anyhow,Context};
use std::net::{ToSocketAddrs,SocketAddr};
use std::str::FromStr;
use log::{debug, error, info, trace, warn};

use crate::cli::Cli;
use log4rs::append::console::ConsoleAppender;
use log4rs::Config;
use log4rs::config::{Appender, Root};
use log4rs::encode::pattern::PatternEncoder;

type Result<T> = anyhow::Result<T, anyhow::Error>;

#[allow(unused)]
pub fn print_type_of<T>(_: &T) {
    println!("{}", std::any::type_name::<T>())
}

/*
pub fn init_log_env(level: LevelFilter) {

    let mut builder = env_logger::Builder::new();

    builder.format(|buf, record| {
        writeln!(buf, "{} [{:4}] [{}:{}] {:>5}: {} ", Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                 std::thread::current().name().or(Some("unknown")).unwrap(),
                 record.file().unwrap(),
                 record.line().unwrap(),
                 record.level(),
                 record.args())
    });
    builder.filter_level(level);
    builder.init();
}
*/
pub fn init_log(cli: &Cli) -> Result<()> {
    if let Some(ref log_config) = cli.log_config {
        log4rs::init_file(log_config,Default::default())?;
        println!("setup config for logging with {}", log_config.to_str().unwrap());
        info!("START with log configured from {}", log_config.to_str().unwrap());
        Ok(())
    } else {
        println!("using default console logging");
        let log_level = cli.log_level.unwrap_or(LevelFilter::Info);
        let pat = Box::new(PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S%.3f %Z)(utc)} [{l:<5}] {f}:{L} - {m} {n}"));
        let stdout = ConsoleAppender::builder().encoder(pat).build();
        let config = Config::builder()
            .appender(Appender::builder().build("stdout", Box::new(stdout)))
            .build(Root::builder().appender("stdout").build(log_level))
            .unwrap();
        let handle = log4rs::init_config(config).unwrap();
        Ok(())
    }
}

pub fn to_log_level(s: &str) -> anyhow::Result<LevelFilter, anyhow::Error> {
    match s {
        "off" | "o" => Ok(LevelFilter::Off),
        "error" | "e"  => Ok(LevelFilter::Error),
        "warn" | "w" => Ok(LevelFilter::Warn),
        "info" | "i" => Ok(LevelFilter::Info),
        "debug" | "d" => Ok(LevelFilter::Debug),
        "trace" | "t" => Ok(LevelFilter::Trace),
        _ => Err(anyhow::anyhow!("Error for log level: must be one of off, o, error, e, warn, w, info, i, debug, d, trace, t but got {}", &s))
    }
}

pub fn to_duration(s: &str) -> Result<Duration> {
    let mut num = String::new();
    let mut sum_secs = 0u64;
    for c in s.chars() {
        if c >= '0' && c <='9' {
            num.push(c);
        } else {
            let s = num.parse::<u64>().with_context(|| format!("cannot parse number {} inside duration {}", &num, &s))?;
            num.clear();
            match c {
                's' => sum_secs += s,
                'm' => sum_secs += s*60,
                'h' => sum_secs += s * 3600,
                'd' => sum_secs += s*3600*24,
                'w' => sum_secs += s*3600*24*7,
                _ => Err(anyhow!("Cannot interpret {} as a time unit inside duration {}", c, &s))?,
            }
        }
    }
    if num.len() > 0 {
        sum_secs += num.parse::<u64>().with_context(|| format!("cannot parse number {} inside duration {}", &num, &s))?;
    }
    Ok(Duration::from_secs(sum_secs))
}


const GREEK_SUFFIXES: &[&str] = &["", "K", "M", "G", "T", "P", "E", "Z", "Y"];
pub fn greek(v: f64) -> String {
    let mut number = v;
    let mut multi = 0;

    while number >= 1000.0 && multi < GREEK_SUFFIXES.len() - 1 {
        multi += 1;
        number /= 1024.0;
    }

    let mut s = format!("{}", number);
    s.truncate(4);
    if s.ends_with('.') {
        s.pop();
    }
    if s.len() < 4 { s.push(' '); }

    return format!("{:<5}{}", s, GREEK_SUFFIXES[multi]);
}


pub fn to_size_u64(s: &str) -> Result<u64> {
    let mut num = String::new();
    let mut bytes = 0u64;
    for c in s.chars() {
        if c >= '0' && c <='9' {
            num.push(c);
        } else {
            let s = num.parse::<u64>().with_context(|| format!("cannot parse number {} inside duration {}", &num, &s))?;
            num.clear();
            match c {
                'k' | 'K'  => bytes += s * 1024,
                'm' | 'M'  => bytes += s * (1024*1024),
                'g' | 'G' => bytes += s * (1024*1024*1024),
                't' | 'T' => bytes += s * (1024*1024*1024*1024),
                'p' | 'P' => bytes += s * (1024*1024*1024*1024*1024),
                _ => Err(anyhow!("Cannot interpret {} as a bytes unit inside size {}", c, &s))?,
            }
        }
    }
    if num.len() > 0 {
        bytes += num.parse::<u64>().with_context(|| format!("cannot parse number {} inside size {}", &num, &s))?;
    }
    Ok(bytes)
}

pub fn to_size_usize(s: &str) -> Result<usize> {
    let sz = to_size_u64(s)?;
    return Ok(sz as usize);
}

pub fn str_to_socketaddr(s: &str) -> Result<SocketAddr> {
    //if let Some(s) = s {
    use std::net::SocketAddr;
    use std::net::IpAddr;
    match s.parse() {
        Ok(soc) => Ok(soc),
        Err(e) => {
            let mut buf = String::from(s);
            buf.push_str(":5150");
            match buf.as_str().to_socket_addrs() {
                Ok(mut soc_itr) => {
                    if let Some(soc) = soc_itr.next() {
                        Ok(soc)
                    } else {
                        Err(anyhow!("empty result from DNS lookup for: {}", &s))
                    }
                },
                Err(e) => Err(anyhow!("Unable to get socket address from {} because \"{}\"", &s, e)),
            }
        }
    }
}

pub fn sleep_until_even_interval(now: Option<&SystemTime>, interval: &Duration) {
    let until_next = compute_until_even_interval_nanos(now, interval);
    std::thread::sleep(until_next);
}

pub fn compute_until_even_interval_nanos(now: Option<&SystemTime>, interval: &Duration) -> Duration {
    let now = now.unwrap_or(&SystemTime::now());
    let now_nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("cannot get system time")
        .as_nanos();// / dur.as_nanos();

    Duration::from_nanos((((now_nanos / interval.as_nanos() + 1) *  interval.as_nanos() - now_nanos) as u64))
}