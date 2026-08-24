use netdev::interface::state::OperState;
use netdev::interface::types::InterfaceType;
use std::net::Ipv4Addr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicKind {
    Wifi,
    Ethernet,
    Tunnel,
    Virtual,
    Other,
}

impl NicKind {
    fn rank(self) -> u8 {
        match self {
            Self::Wifi => 0,
            Self::Ethernet => 1,
            Self::Tunnel => 2,
            Self::Virtual => 3,
            Self::Other => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nic {
    pub name: String,
    pub ip: String,
    pub kind: NicKind,
}

fn joined_metadata(name: &str, friendly_name: Option<&str>, description: Option<&str>) -> String {
    format!(
        "{} {} {}",
        name,
        friendly_name.unwrap_or_default(),
        description.unwrap_or_default()
    )
    .to_ascii_lowercase()
}

fn is_local_only(metadata: &str) -> bool {
    [
        "docker",
        "cni",
        "veth",
        "virbr",
        "vmnet",
        "vboxnet",
        "virtualbox",
        "wsl",
        "default switch",
    ]
    .iter()
    .any(|needle| metadata.contains(needle))
}

fn looks_like_tunnel(metadata: &str) -> bool {
    let first = metadata.split_whitespace().next().unwrap_or_default();
    first.starts_with("tun")
        || first.starts_with("utun")
        || first.starts_with("tap")
        || first.starts_with("wg")
        || first.starts_with("zt")
        || first.starts_with("ppp")
        || [
            "tailscale",
            "zerotier",
            "zero tier",
            "wireguard",
            "wintun",
            "ipsec",
            " tunnel",
        ]
        .iter()
        .any(|needle| metadata.contains(needle))
}

fn looks_like_wifi(metadata: &str) -> bool {
    metadata.contains("wlan")
        || metadata.contains("wifi")
        || metadata.contains("wi-fi")
        || metadata.contains("airport")
        || metadata.split_whitespace().next() == Some("en0")
}

fn classify(if_type: InterfaceType, metadata: &str) -> NicKind {
    if looks_like_tunnel(metadata)
        || matches!(
            if_type,
            InterfaceType::Ppp
                | InterfaceType::Slip
                | InterfaceType::Tunnel
                | InterfaceType::ProprietaryVirtual
        )
    {
        return NicKind::Tunnel;
    }
    if looks_like_wifi(metadata) || if_type == InterfaceType::Wireless80211 {
        return NicKind::Wifi;
    }
    if matches!(
        if_type,
        InterfaceType::Ethernet
            | InterfaceType::Ethernet3Megabit
            | InterfaceType::FastEthernetT
            | InterfaceType::FastEthernetFx
            | InterfaceType::GigabitEthernet
    ) {
        return NicKind::Ethernet;
    }
    if matches!(
        if_type,
        InterfaceType::Bridge | InterfaceType::PeerToPeerWireless
    ) {
        return NicKind::Virtual;
    }
    NicKind::Other
}

fn is_shared_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn is_candidate_v4(ip: Ipv4Addr, kind: NicKind) -> bool {
    if ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
    {
        return false;
    }
    ip.is_private() || is_shared_v4(ip) || matches!(kind, NicKind::Tunnel | NicKind::Virtual)
}

pub fn list_nics() -> Vec<Nic> {
    let mut out = Vec::new();
    for interface in netdev::get_interfaces() {
        if interface.oper_state != OperState::Up {
            continue;
        }
        let metadata = joined_metadata(
            &interface.name,
            interface.friendly_name.as_deref(),
            interface.description.as_deref(),
        );
        if is_local_only(&metadata) {
            continue;
        }
        let kind = classify(interface.if_type, &metadata);
        let name = interface
            .friendly_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(interface.name);
        for network in interface.ipv4 {
            let ip = network.addr();
            if !is_candidate_v4(ip, kind) {
                continue;
            }
            let ip = ip.to_string();
            if out.iter().any(|nic: &Nic| nic.ip == ip) {
                continue;
            }
            out.push(Nic {
                name: name.clone(),
                ip,
                kind,
            });
        }
    }
    out.sort_by(|a, b| {
        (a.kind.rank(), a.name.to_ascii_lowercase(), &a.ip).cmp(&(
            b.kind.rank(),
            b.name.to_ascii_lowercase(),
            &b.ip,
        ))
    });
    out
}

pub fn default_ip(nics: &[Nic]) -> Option<String> {
    nics.first().map(|n| n.ip.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_physical_tunnel_and_virtual_interfaces() {
        assert_eq!(
            classify(InterfaceType::Wireless80211, "wi-fi"),
            NicKind::Wifi
        );
        assert_eq!(
            classify(InterfaceType::Ethernet, "ethernet"),
            NicKind::Ethernet
        );
        assert_eq!(classify(InterfaceType::Tunnel, "utun4"), NicKind::Tunnel);
        assert_eq!(
            classify(InterfaceType::Ethernet, "tailscale"),
            NicKind::Tunnel
        );
        assert_eq!(
            classify(InterfaceType::Bridge, "bridge100"),
            NicKind::Virtual
        );
    }

    #[test]
    fn hides_container_and_vm_only_interfaces() {
        for metadata in [
            "docker0",
            "cni0",
            "veth123",
            "vmnet8",
            "vboxnet0",
            "virtualbox host-only",
            "vethernet default switch",
            "wsl",
        ] {
            assert!(is_local_only(metadata), "{metadata}");
        }
        for metadata in ["en0", "ethernet", "tailscale", "utun3", "bridge100"] {
            assert!(!is_local_only(metadata), "{metadata}");
        }
    }

    #[test]
    fn accepts_lan_shared_and_tunnel_addresses() {
        assert!(is_candidate_v4(
            Ipv4Addr::new(192, 168, 1, 2),
            NicKind::Wifi
        ));
        assert!(is_candidate_v4(
            Ipv4Addr::new(100, 64, 0, 1),
            NicKind::Tunnel
        ));
        assert!(is_candidate_v4(
            Ipv4Addr::new(100, 127, 255, 254),
            NicKind::Tunnel
        ));
        assert!(is_candidate_v4(
            Ipv4Addr::new(203, 0, 113, 7),
            NicKind::Tunnel
        ));
        assert!(!is_candidate_v4(
            Ipv4Addr::new(8, 8, 8, 8),
            NicKind::Ethernet
        ));
        assert!(!is_candidate_v4(
            Ipv4Addr::new(127, 0, 0, 1),
            NicKind::Tunnel
        ));
        assert!(!is_candidate_v4(
            Ipv4Addr::new(169, 254, 1, 1),
            NicKind::Tunnel
        ));
    }

    #[test]
    fn current_snapshot_has_unique_candidate_ips() {
        let mut seen = std::collections::HashSet::new();
        for nic in list_nics() {
            assert!(seen.insert(nic.ip.clone()), "duplicate IP: {}", nic.ip);
            let ip = nic.ip.parse::<Ipv4Addr>().unwrap();
            assert!(is_candidate_v4(ip, nic.kind));
        }
    }

    #[test]
    fn wifi_then_ethernet_then_tunnel_order_selects_wifi() {
        let mut nics = vec![
            Nic {
                name: "Tunnel".into(),
                ip: "100.64.0.5".into(),
                kind: NicKind::Tunnel,
            },
            Nic {
                name: "Ethernet".into(),
                ip: "10.0.0.5".into(),
                kind: NicKind::Ethernet,
            },
            Nic {
                name: "Wi-Fi".into(),
                ip: "192.168.1.2".into(),
                kind: NicKind::Wifi,
            },
        ];
        nics.sort_by_key(|n| n.kind.rank());
        assert_eq!(default_ip(&nics).unwrap(), "192.168.1.2");
    }
}
