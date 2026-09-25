//! Settings from the environment, parsed through `Config::from_lookup` so the
//! process environment is never touched.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use guhit_relay::Config;

fn parse(vars: &[(&str, &str)]) -> Result<Config, String> {
    Config::from_lookup(|name| vars.iter().find(|(n, _)| *n == name).map(|(_, v)| v.to_string()))
}

#[test]
fn defaults_are_the_documented_ones() {
    let c = parse(&[]).unwrap();
    assert_eq!(c, Config::default());
    assert_eq!(c.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    assert_eq!(c.port, 8080);
    assert_eq!(c.max_rooms, 5000);
    assert_eq!(c.max_connections, 20_000);
    assert_eq!(c.max_per_ip, 64);
    assert_eq!(c.max_rooms_per_ip, 8);
    assert_eq!(c.max_waiting_per_room, 8);
    assert_eq!(c.max_pairs_per_room, 32);
    assert_eq!(c.rate, 4_194_304);
    assert_eq!(c.burst, 16_777_216);
    assert_eq!(c.client_ip_header, None);
    assert_eq!(c.hello_timeout, Duration::from_secs(10));
    assert_eq!(c.accept_timeout, Duration::from_secs(10));
    assert_eq!(c.ping_interval, Duration::from_secs(20));
    assert_eq!(c.idle_timeout, Duration::from_secs(60));
    assert_eq!(c.max_frame, 256 * 1024);
}

#[test]
fn every_variable_is_read() {
    let c = parse(&[
        ("RELAY_BIND", "127.0.0.1"),
        ("PORT", "1470"),
        ("RELAY_MAX_ROOMS", "10"),
        ("RELAY_MAX_CONNECTIONS", "11"),
        ("RELAY_MAX_PER_IP", "12"),
        ("RELAY_MAX_ROOMS_PER_IP", "13"),
        ("RELAY_MAX_WAITING_PER_ROOM", "14"),
        ("RELAY_MAX_PAIRS_PER_ROOM", "15"),
        ("RELAY_RATE", "16"),
        ("RELAY_BURST", "17"),
        ("RELAY_CLIENT_IP_HEADER", "fly-client-ip"),
    ])
    .unwrap();
    assert_eq!(c.bind, IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(c.port, 1470);
    assert_eq!(c.max_rooms, 10);
    assert_eq!(c.max_connections, 11);
    assert_eq!(c.max_per_ip, 12);
    assert_eq!(c.max_rooms_per_ip, 13);
    assert_eq!(c.max_waiting_per_room, 14);
    assert_eq!(c.max_pairs_per_room, 15);
    assert_eq!(c.rate, 16);
    assert_eq!(c.burst, 17);
    assert_eq!(c.client_ip_header.as_deref(), Some("fly-client-ip"));
    // The protocol's timeouts are not settings.
    assert_eq!(c.idle_timeout, Config::default().idle_timeout);
}

#[test]
fn empty_is_unset_and_spaces_are_trimmed() {
    let c = parse(&[("RELAY_BIND", ""), ("PORT", " 9000 "), ("RELAY_CLIENT_IP_HEADER", "  ")]).unwrap();
    assert_eq!(c.bind, Config::default().bind);
    assert_eq!(c.port, 9000);
    assert_eq!(c.client_ip_header, None);
}

#[test]
fn bind_takes_ipv4_and_ipv6() {
    assert_eq!(parse(&[("RELAY_BIND", "::")]).unwrap().bind, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
    assert_eq!(parse(&[("RELAY_BIND", "::1")]).unwrap().bind, IpAddr::V6(Ipv6Addr::LOCALHOST));
    assert_eq!(parse(&[("RELAY_BIND", "10.1.2.3")]).unwrap().bind, IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)));
}

#[test]
fn a_bad_value_is_an_error_that_names_the_variable() {
    let bad = [
        ("RELAY_BIND", "localhost"),
        ("RELAY_BIND", "0.0.0.0:8080"),
        ("PORT", "70000"),
        ("PORT", "http"),
        ("RELAY_MAX_ROOMS", "-1"),
        ("RELAY_MAX_CONNECTIONS", "1e4"),
        ("RELAY_MAX_PER_IP", "sixty"),
        ("RELAY_MAX_ROOMS_PER_IP", "8.5"),
        ("RELAY_MAX_WAITING_PER_ROOM", "x"),
        ("RELAY_MAX_PAIRS_PER_ROOM", "32 pairs"),
        ("RELAY_RATE", "4MB"),
        ("RELAY_RATE", "0"),
        ("RELAY_BURST", "-5"),
        ("RELAY_CLIENT_IP_HEADER", "not a header"),
        ("RELAY_CLIENT_IP_HEADER", "x-forwarded-for:"),
    ];
    for (name, value) in bad {
        let err = parse(&[(name, value)]).expect_err(&format!("{name}={value} should be refused"));
        assert!(err.contains(name), "{name}={value}: {err}");
    }
}

#[test]
fn header_names_are_lowercased() {
    let c = parse(&[("RELAY_CLIENT_IP_HEADER", "X-Forwarded-For")]).unwrap();
    assert_eq!(c.client_ip_header.as_deref(), Some("x-forwarded-for"));
}

#[test]
fn burst_may_be_zero() {
    assert_eq!(parse(&[("RELAY_BURST", "0")]).unwrap().burst, 0);
}
