#![cfg(test)]

use std::net::IpAddr;

use super::*;

macro_rules! assert_contains {
    ($input:expr, $item:expr $(,)?) => {{
        if !$input.contains(&$item) {
            panic!(
                "assertion failed: input does not contain item\n  input: {:?}\n  item: {:?}",
                $input, $item
            );
        }
    }};
    ($input:expr, $item:expr, $($arg:tt)+) => {{
        if !$input.contains(&$item) {
            panic!($($arg)+);
        }
    }};
}

#[test]
fn test_load_config_missing_file() {
    // missing source field
    let err = Config::from_str("").err().unwrap();
    assert_contains!(err.to_string(), "missing field `sources`");

    // empty source field
    let err = Config::from_str("sources: []").err().unwrap();
    assert_contains!(err.to_string(), "`source` cannot be empty");

    // empty unit in journal source
    let err = Config::from_str(
        r#"
                sources:
                  - type: journal
                    unit: ""
                    rules:
                      - name: ssh
                        ban_duration: 1h
                        window: 1h
                        max_attempts: 5
                        ban_action: nftables-allports
                        pattern:
                          - Failed
                          - password
            "#,
    )
    .err()
    .unwrap();
    // TODO
    assert_contains!(err.to_string(), "unit");
}

#[test]
fn test_load_config_ok() {
    let raw = r#"
            worker_threads: 2
            whitelists:
              - 192.168.0.0/16
              - 101.102.103.104/32
            sources:
              - type: journal
                unit: sshd
                rules:
                  - name: ssh
                    ban_duration: 1h
                    window: 1h
                    max_attempts: 5
                    ban_action: nftables-allports
                    pattern:
                      - Failed
                      - password
        "#;

    let config: Config = Config::from_str(raw).expect("Failed to parse config");
    assert_eq!(config.worker_threads, Some(2));
    assert_eq!(
        config.whitelists,
        Some(vec!["192.168.0.0/16".parse().unwrap(), "101.102.103.104/32".parse().unwrap(),])
    );

    match &config.sources[0] {
        SourceConfig::Journal { rules, .. } => {
            assert_eq!(rules[0].ban_duration, Duration::from_secs(3600));
            assert_eq!(rules[0].ban_action, "nftables-allports".to_string());
        }
        _ => panic!("Expected Journal source"),
    }
}

#[test]
fn test_ipnet() {
    let cases = vec![
        (
            "101.102.103.104/32",
            vec![
                ("101.102.103.104", true),
                ("127.0.0.1", false),
                ("101.102.103.0", false),
                ("101.102.103.103", false),
                ("101.102.103.105", false),
            ],
        ),
        (
            "101.102.103.104/24",
            vec![("101.102.103.104", true), ("101.102.103.103", true), ("101.102.103.105", true)],
        ),
    ];
    for (cidr, tests) in cases {
        let ipnet: IpNet = cidr.parse().unwrap();
        for (ip_str, expected) in tests {
            let ip = ip_str.parse::<IpAddr>().unwrap();
            assert_eq!(ipnet.contains(&ip), expected, "CIDR: {}, IP: {}", cidr, ip_str);
        }
    }
}
