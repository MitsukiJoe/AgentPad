use std::net::Ipv4Addr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nic {
    pub name: String,
    pub ip: String,
    pub wifi: bool,
}

pub fn is_virtual(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n == "lo" || n.starts_with("lo0") || n.starts_with("lo1") || n.starts_with("lo2") {
        return true;
    }
    [
        "tun",
        "utun",
        "tap",
        "wg",
        "tailscale",
        "ipsec",
        "ppp",
        "awdl",
        "llw",
        "bridge",
        "vnic",
        "vmnet",
        "vboxnet",
        "docker",
        "cni",
        "veth",
        "virbr",
        "zt",
    ]
    .iter()
    .any(|needle| n.contains(needle))
}

pub fn is_wifi(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("wlan")
        || n.contains("wifi")
        || n.contains("wi-fi")
        || n.contains("airport")
        || n == "en0"
}

pub fn is_lan_v4(ip: Ipv4Addr) -> bool {
    ip.is_private() && !ip.is_loopback() && !ip.is_link_local()
}

pub fn list_nics() -> Vec<Nic> {
    let mut out = Vec::new();
    let Ok(addrs) = if_addrs::get_if_addrs() else {
        return out;
    };
    for a in addrs {
        if is_virtual(&a.name) {
            continue;
        }
        let std::net::IpAddr::V4(ip) = a.ip() else {
            continue;
        };
        if !is_lan_v4(ip) {
            continue;
        }
        out.push(Nic {
            wifi: is_wifi(&a.name),
            name: a.name,
            ip: ip.to_string(),
        });
    }
    out.sort_by_key(|n| if n.wifi { 0u8 } else { 1 });
    out
}

pub fn default_ip(nics: &[Nic]) -> Option<String> {
    nics.iter()
        .find(|n| n.wifi)
        .or_else(|| nics.first())
        .map(|n| n.ip.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_vpn_and_loopback_names() {
        for n in [
            "utun0",
            "utun2",
            "tun0",
            "tap0",
            "wg0",
            "tailscale0",
            "lo0",
            "awdl0",
            "bridge100",
        ] {
            assert!(is_virtual(n), "{n}");
        }
        for n in ["en0", "en1", "eth0", "wlan0", "Ethernet"] {
            assert!(!is_virtual(n), "{n}");
        }
    }

    #[test]
    fn private_v4_only() {
        assert!(is_lan_v4(Ipv4Addr::new(192, 168, 1, 2)));
        assert!(is_lan_v4(Ipv4Addr::new(10, 0, 0, 5)));
        assert!(is_lan_v4(Ipv4Addr::new(172, 16, 0, 1)));
        assert!(!is_lan_v4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!is_lan_v4(Ipv4Addr::new(8, 8, 8, 8)));
        assert!(!is_lan_v4(Ipv4Addr::new(169, 254, 1, 1)));
    }

    #[test]
    fn wifi_preferred() {
        let nics = vec![
            Nic {
                name: "en1".into(),
                ip: "10.0.0.5".into(),
                wifi: false,
            },
            Nic {
                name: "en0".into(),
                ip: "192.168.1.2".into(),
                wifi: true,
            },
        ];
        assert_eq!(default_ip(&nics).unwrap(), "192.168.1.2");
        assert!(is_wifi("en0"));
        assert!(is_wifi("wlan0"));
    }
}
