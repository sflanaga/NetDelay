#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]

mod util;
mod cli;

use std::path::PathBuf;
use structopt::StructOpt;
use std::time::{Instant, Duration};
use anyhow::{anyhow, Context};
use log::{debug, error, info, trace, warn};
use log::LevelFilter;
use std::net::{TcpListener, TcpStream, SocketAddr, IpAddr, Ipv4Addr};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use util::{to_log_level, to_duration, to_size_usize};
use std::str::FromStr;
use std::thread::spawn;
use core::mem;
use humantime::parse_duration;
use lazy_static::lazy_static;
use crate::cli::Cli;
use serde::{Serialize, Deserialize, Serializer};
use std::sync::mpsc::RecvTimeoutError::Timeout;
use std::ops::Deref;
use std::borrow::BorrowMut;
use std::fmt::Formatter;


struct _Stat {
    echos: u64,
    tot_time_ms: Duration,
    max_time_ms: Duration,
}

#[derive(Clone)]
struct Stat {
    inner: Arc<Mutex<_Stat>>,
}

impl _Stat {
    fn zero(self: &mut Self) {
        self.max_time_ms=Duration::from_secs(0);
        self.tot_time_ms=Duration::from_secs(0);
        self.echos=0;
    }
}

impl Stat {
    pub fn new() -> Self {
        Stat {
            inner: Arc::new(Mutex::new(_Stat {
                echos: 0,
                tot_time_ms: Duration::from_secs(0),
                max_time_ms: Duration::from_secs(0),
            }))
        }
    }

    pub fn update(self: &mut Self, time_ms: Duration) {
        let mut lock = self.inner.lock().expect("Unable to update State at lock");
        lock.echos += 1;
        lock.tot_time_ms += time_ms;
        if time_ms > lock.max_time_ms {
            lock.max_time_ms = time_ms;
        }
    }
    pub fn snap_shot(self: &mut Self) -> (u64, Duration, Duration) {
        let mut lock = self.inner.lock().expect("Unable to take snap_shot of Stat at lock");
        let (echos, tot_time_ms, max_time_ms) = (lock.echos,lock.tot_time_ms,lock.max_time_ms);
        lock.zero();
        (echos, tot_time_ms, max_time_ms)
    }
}

lazy_static! {
    pub static ref STOP_TICKER: AtomicBool = AtomicBool::new(false);

    pub static ref COND_STOP: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
}

type Result<T> = anyhow::Result<T, anyhow::Error>;

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct TimePacket {
    #[serde(with = "serde_millis")]
    send_time: std::time::Instant,
    #[serde(with = "serde_millis")]
    resp_time: Option<std::time::Instant>,
}

impl TimePacket {
    pub fn new() -> Self {
        TimePacket {
            send_time: std::time::Instant::now(),
            resp_time: None,
        }
    }
}


fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {:?}", &err);
        error!("Error: {:?}", &err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli: Cli = Cli::from_args();

    util::init_log(&cli).context("initializing log configuration")?;
    if let Some(ref socket_addr) = cli.server {
        let mut socket_addr = if let Some(ref socket_addr) = socket_addr {
            util::str_to_socketaddr(socket_addr)?
        } else {
            SocketAddr::new(IpAddr::from(Ipv4Addr::new(0, 0, 0, 0)), cli.port)
        };
        socket_addr.set_port(cli.port);

        info!("server listening to {}", &socket_addr);
        let listener = TcpListener::bind(socket_addr).with_context(|| format!("not a valid IP address: {}", &socket_addr))?;
        // accept connections and process them serially
        let mut serv_count = 0;
        for stream in listener.incoming() {
            let stream = stream?;
            serv_count += 1;
            let cli = cli.clone();
            std::thread::Builder::new()
                .name(format!("serv_{}", serv_count))
                .spawn(move || {
                    server_thread_handler(stream, &cli);
                }).unwrap();
        }
    } else if let Some(mut socker_addr) = cli.client {
        let mut stat = Stat::new();
        if let Some(ticker_interval) = cli.ticker_interval {
            let cli = cli.clone();
            spawn_ticker(&cli, ticker_interval, stat.clone());
        }
        socker_addr.set_port(cli.port);
        client_forever(&cli, stat, &socker_addr);
        stop_ticker();

    } else {
        return Err(anyhow!("Error - either server or client must be specified"))?;
    }

    Ok(())
}

fn server_thread_handler(mut stream: TcpStream, cli: &Cli) {
    match server(stream, &cli) {
        Err(e) => warn!("client thread error: {}", single_line_error(&e)),
        Ok(()) => {}
    }
}

fn single_line_error(e: &anyhow::Error) -> String {
    let mut s = format!("{:?}", e);
    s = s.replace("\n", " ");
    s = s.replace("    ", " ");
    s
}

fn server(mut stream: TcpStream, cli: &Cli) -> Result<()> {
    stream.set_read_timeout(Some(cli.timeout_socket)).context("setting read timeout")?;
    stream.set_write_timeout(Some(cli.timeout_socket)).context("setting write timeout")?;
    stream.set_nodelay(true).context("setting nodelay of server socket");
    let client_addr = stream.peer_addr().context("Unable to get peer_address after incoming connection")?;
    info!("Connection from: {:?}", &client_addr);

    loop {
        let mut tp: TimePacket = bincode::deserialize_from(&stream).context(format!("with client IP {} at read", client_addr))?;
        tp.resp_time = Some(std::time::Instant::now());
        bincode::serialize_into(&stream, &tp).context(format!("with client IP {} at write", client_addr))?;
        stream.flush();
        if let Some(ref dur) = cli.interval {
            std::thread::sleep(*dur);
        }
        debug!("Packet sent {:#?}", &tp);
    }
    return Ok(());
}


fn build_client_stream(cli: &Cli, socker_addr: &SocketAddr) -> Result<TcpStream> {
    let mut stream = TcpStream::connect_timeout(&socker_addr, cli.timeout_socket).context("setting connect timeout of client socket")?;
    stream.set_read_timeout(Some(cli.timeout_socket)).context("setting read timeout of client socket")?;
    stream.set_write_timeout(Some(cli.timeout_socket)).context("setting write timeout of client socket")?;
    stream.set_nodelay(true).context("setting nodelay").context("setting nodelay of client socket");

    Ok(stream)
}

fn client_forever(cli: &Cli, mut stat: Stat, socker_addr: &SocketAddr) {
    loop {
        info!("client trying to connect to {}", &socker_addr);
        let mut stream = loop {
            match build_client_stream(&cli, &socker_addr) {
                Err(e) => {
                    error!("Unable to build client stream: {}", single_line_error(&e));
                    info!("Will attempt to reconnect after a short break of {} seconds", cli.break_time.as_secs());
                },
                Ok(s) => break s,
            }
        };
        info!("client connected to {}", &socker_addr);

        match client(stream, &cli, stat.clone()) {
            Err(e) => {
                error!("Error after connection: {}", single_line_error(&e));
                info!("Will attempt to reconnect after a short break of {} seconds", cli.break_time.as_secs());
                std::thread::sleep(cli.break_time);
            },
            Ok(()) => break,
        }

    }
}

fn client(mut stream: TcpStream, cli: &Cli, mut stat: Stat) -> Result<()> {
    loop {
        let server_addr = stream.peer_addr().context("Unable to get peer_address after incoming connection")?;

        let tp_sent = TimePacket::new();
        bincode::serialize_into(&stream, &tp_sent).context(format!("with IP server {} at read", server_addr))?;
        let tp_recv: TimePacket = bincode::deserialize_from(&stream).context(format!("with IP server {} at write", server_addr))?;
        let dur = tp_recv.send_time.elapsed();
        debug!("echo {:.3} ms", dur.as_secs_f64()*1000f64);

        stat.update(dur);
        // info!("post echo {} ms", dur.as_millis());
        if dur > cli.warn_threshold {
            warn!("broke threshold - echo time: {:?}", &dur);
        } else if dur > cli.info_threshold {
            info!("broke info threshold - echo time: {:?}", &dur);
        }
        if let Some(ref dur) = cli.interval {
            std::thread::sleep(*dur);
        }
        debug!("Return packet: {:?}", &dur);
    }
    Ok(())
}

fn stop_ticker() {
    let mut lock = COND_STOP.0.lock().unwrap();
    *lock = true;
    COND_STOP.1.notify_all();
}

fn spawn_ticker(cli: &Cli, dur: Duration, mut stat: Stat) {
    {
        let mut lock = COND_STOP.0.lock().unwrap();
        *lock = false;
    }

    let cli = cli.clone();
    std::thread::Builder::new()
        .name("ticker".to_string())
        .spawn(move || {
            info!("stat ticker started");
            loop {
                let mut tot_ticks = 0;
                {
                    let lock = COND_STOP.0.lock().unwrap();
                    if !*lock {
                        let res = COND_STOP.1.wait_timeout(lock, dur).unwrap();
                        if *res.0 {
                            info!("stopping on check of condition during or interrupted sleep");
                            break;
                        }
                    } else {
                        debug!("stopping on initial check of condition before sleep");
                        break;
                    };
                }
                if STOP_TICKER.load(Ordering::Relaxed) == true {
                    info!("tic stopped");
                    break;
                }
                let (echos, tot_time_ms, max_time_ms) = stat.snap_shot();
                let rate = (echos) as f64 / dur.as_secs() as f64;
                tot_ticks += echos;
                if echos <= 0 {
                    info!("No echo stats to report - no working echos");
                } else {
                    let avg_ms = Duration::from_nanos((tot_time_ms.as_nanos() / echos as u128) as u64);
                    if cli.human_time {
                        info!("echos: {} rate: {} max time: {} avg time: {}", tot_ticks
                              , util::greek(rate)
                              , duration_to_human(&max_time_ms, 2)
                              , duration_to_human(&avg_ms, 2));
                    } else {
                        info!("echos: {} rate: {} max time: {:.3}ms avg time: {:.3}ms", tot_ticks
                              , util::greek(rate)
                              , max_time_ms.as_secs_f64() * 1000f64
                              , avg_ms.as_secs_f64() * 1000f64);
                    }
                }
            }
        })
        .unwrap();
}

pub fn duration_to_human(dur: &Duration, prec: u32) -> String {
    const TIME_UNITS: &[(u128,&str)] = &[(1_000_000_000, "s"), (1_000_000, "ms"),(1_000, "u"),(1, "ns")];
    let mut num = dur.as_nanos();
    let mut str = String::new();
    let mut prec_remaining = prec;
    for (i,conv) in TIME_UNITS.iter().enumerate() {
        if num > conv.0 {
            let m = num / conv.0;
            let this_num = m*conv.0;
            if this_num < 11 {
                prec_remaining -= 1;
            }
            num -= m * conv.0;
            str.push_str(&format!("{}{}", m, conv.1));
            if prec_remaining <= 0 {
                break;
            }
        }
    }
    return str;
}

struct MyDuration(Duration);

impl Into<Duration> for MyDuration {
    fn into(self) -> Duration {
        self.0
    }
}

impl std::fmt::Display for MyDuration {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(),std::fmt::Error> {

        const TIME_UNITS: &[(u128,&str)] = &[(1_000_000_000, "s"), (1_000_000, "ms"),(1_000, "u"),(1, "ns")];
        let mut num = self.0.as_nanos();
        let mut str = String::new();
        let mut prec_remaining = if let Some(prec) = f.precision() {
             prec
        } else {
            2
        };
        for (i,conv) in TIME_UNITS.iter().enumerate() {
            if num > conv.0 {
                let m = num / conv.0;
                num -= m * conv.0;
                write!(f, "{}{}", m, conv.1);
                prec_remaining -= 1;
                if prec_remaining <= 0 {
                    break;
                }
            }
        }
        Ok(())
    }
}

