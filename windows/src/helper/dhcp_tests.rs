//! The DHCP server's answers, checked.

use super::*;

fn request(kind: u8, requested: Option<Ip>) -> Vec<u8> {
    let mut p = vec![0u8; 240];
    (p[0], p[1], p[2]) = (1, 1, 6);
    p[4..8].copy_from_slice(&[1, 2, 3, 4]);
    p[28..34].copy_from_slice(&[0xd2, 0x4f, 0x91, 0xaa, 0x10, 0x3c]);
    p[236..240].copy_from_slice(&COOKIE);
    p.extend_from_slice(&[53, 1, kind]);
    if let Some(ip) = requested {
        p.extend_from_slice(&[50, 4]);
        p.extend_from_slice(&ip);
    }
    p.extend_from_slice(&[12, 5, b'P', b'h', b'o', b'n', b'e', 255]);
    p
}

#[test]
fn offers_then_acks_in_range() {
    let net = Network { dhcp_from: 50, dhcp_to: 60, ..Default::default() };
    let gw = [192, 168, 137, 1];
    let mut leases = Leases::default();
    let discover = parse(&request(DISCOVER, None)).unwrap();
    assert_eq!(discover.hostname.as_deref(), Some("Phone"));
    let (kind, ip) = answer(&discover, &net, gw, &mut leases, 1000).unwrap();
    assert_eq!((kind, ip), (OFFER, [192, 168, 137, 50]));
    let req = parse(&request(REQUEST, Some(ip))).unwrap();
    assert_eq!(answer(&req, &net, gw, &mut leases, 1000), Some((ACK, ip)));
    let wrong = parse(&request(REQUEST, Some([192, 168, 137, 99]))).unwrap();
    assert_eq!(answer(&wrong, &net, gw, &mut leases, 1000).map(|a| a.0), Some(NAK));
    let packet = reply(&req, ACK, ip, gw, &net);
    assert_eq!(packet.len(), 20 + 8 + 300);
    assert_eq!(&packet[16..20], &[255; 4]);
}

#[test]
fn fixed_address_wins() {
    let mut net = Network::default();
    net.fixed.insert("d2:4f:91:aa:10:3c".into(), "192.168.137.77".into());
    let req = parse(&request(DISCOVER, None)).unwrap();
    let got = answer(&req, &net, [192, 168, 137, 1], &mut Leases::default(), 0);
    assert_eq!(got, Some((OFFER, [192, 168, 137, 77])));
}
