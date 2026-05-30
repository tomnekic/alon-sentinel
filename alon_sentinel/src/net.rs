use std::net::IpAddr;

pub fn check_ip_is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation())
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return check_ip_is_public(IpAddr::V4(v4));
            }

            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xffc0 == 0xfe80)
                || (v6.segments()[0] & 0xfe00 == 0xfc00))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_ip_is_public;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn check_ip_is_public_rejects_non_public_ranges() {
        let blocked = [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1)),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V4(Ipv4Addr::BROADCAST),
            IpAddr::V4(Ipv4Addr::new(224, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
            IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)),
            IpAddr::V6(Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 1)),
            IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped()),
            IpAddr::V6(Ipv4Addr::new(10, 0, 0, 1).to_ipv6_mapped()),
            IpAddr::V6(Ipv4Addr::new(169, 254, 1, 1).to_ipv6_mapped()),
        ];

        for ip in blocked {
            assert!(!check_ip_is_public(ip), "{ip} should be blocked");
        }
    }

    #[test]
    fn check_ip_is_public_accepts_public_addresses() {
        let allowed = [
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V6(Ipv4Addr::new(8, 8, 8, 8).to_ipv6_mapped()),
            IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111)),
            IpAddr::V6(Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888)),
        ];

        for ip in allowed {
            assert!(check_ip_is_public(ip), "{ip} should be allowed");
        }
    }
}
