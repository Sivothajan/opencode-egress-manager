//! Minimal SOCKS5 client used for WARP isolated mode, Tor, and custom pools.

use std::{
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use crate::errors::{AppError, Result};

/// Validate a domain target for the SOCKS5 domain address type.
fn socks_domain(host: &str) -> Result<Vec<u8>> {
    let host = ascii_host(host)?;
    if host.len() > 255 {
        return Err(AppError::Process("SOCKS5 domain name is too long".into()));
    }
    Ok(host)
}

async fn read_exact(sock: &mut TcpStream, size: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; size];
    sock.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Establish a TCP tunnel through a SOCKS5 proxy.
///
/// Supports no-auth and username/password auth, and IPv4/IPv6/domain targets.
pub async fn open_socks5_tunnel(
    dest_host: &str,
    dest_port: u16,
    socks_host: &str,
    socks_port: u16,
    connect_timeout: Duration,
    username: &str,
    password: &str,
) -> Result<TcpStream> {
    timeout(connect_timeout, async move {
        let mut sock = TcpStream::connect((socks_host, socks_port))
            .await
            .map_err(|e| AppError::Process(format!("SOCKS5 connect failed: {e}")))?;
        sock.write_all(&[0x05, 0x02, 0x00, 0x02]).await?;
        let greeting = read_exact(&mut sock, 2).await?;
        if greeting[0] != 5 {
            return Err(AppError::Process(
                "SOCKS5 proxy returned an invalid greeting".into(),
            ));
        }
        match greeting[1] {
            0x00 => {}
            0x02 => {
                if username.is_empty() {
                    return Err(AppError::Process(
                        "SOCKS5 proxy requires credentials".into(),
                    ));
                }
                let user = username.as_bytes();
                let pass = password.as_bytes();
                if user.len() > 255 || pass.len() > 255 {
                    return Err(AppError::Process("SOCKS5 credentials are too long".into()));
                }
                let mut auth = vec![0x01, user.len() as u8];
                auth.extend_from_slice(user);
                auth.push(pass.len() as u8);
                auth.extend_from_slice(pass);
                sock.write_all(&auth).await?;
                let resp = read_exact(&mut sock, 2).await?;
                if resp[1] != 0 {
                    return Err(AppError::Process(
                        "SOCKS5 proxy rejected the credentials".into(),
                    ));
                }
            }
            other => {
                return Err(AppError::Process(format!(
                    "SOCKS5 proxy does not offer a supported auth method ({other})"
                )))
            }
        }

        let port_bytes = dest_port.to_be_bytes();
        let mut req = vec![0x05, 0x01, 0x00];
        match parse_target(dest_host) {
            Target::V4(ip) => {
                req.push(0x01);
                req.extend_from_slice(&ip.octets());
            }
            Target::V6(ip) => {
                req.push(0x04);
                req.extend_from_slice(&ip.octets());
            }
            Target::Domain => {
                let host = socks_domain(dest_host)?;
                req.push(0x03);
                req.push(host.len() as u8);
                req.extend_from_slice(&host);
            }
        }
        req.extend_from_slice(&port_bytes);
        sock.write_all(&req).await?;

        let reply = read_exact(&mut sock, 4).await?;
        if reply[1] != 0 {
            return Err(AppError::Process(format!(
                "SOCKS5 proxy connection failed (error code {})",
                reply[1]
            )));
        }
        match reply[3] {
            1 => {
                let _ = read_exact(&mut sock, 6).await?;
            }
            4 => {
                let _ = read_exact(&mut sock, 18).await?;
            }
            3 => {
                let len = read_exact(&mut sock, 1).await?;
                let n = len[0] as usize;
                let _ = read_exact(&mut sock, n + 2).await?;
            }
            other => {
                return Err(AppError::Process(format!(
                    "SOCKS5 proxy returned unknown address type {other}"
                )))
            }
        }
        Ok(sock)
    })
    .await
    .map_err(|_| AppError::Process("SOCKS5 handshake timed out".into()))?
}

enum Target {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
    Domain,
}

fn parse_target(host: &str) -> Target {
    let clean = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(v4) = clean.parse::<Ipv4Addr>() {
        return Target::V4(v4);
    }
    if let Ok(v6) = clean.parse::<Ipv6Addr>() {
        return Target::V6(v6);
    }
    Target::Domain
}

fn ascii_host(host: &str) -> Result<Vec<u8>> {
    if host.is_empty() || host.len() > 253 {
        return Err(AppError::validation("invalid destination host"));
    }
    if !host.is_ascii() {
        // SOCKS5 domain names are IDNA-encoded in theory; most local proxies
        // accept UTF-8. Reject rather than emit a malformed length byte.
        return Err(AppError::validation(
            "non-ASCII destination hosts are not supported by this client",
        ));
    }
    Ok(host.as_bytes().to_vec())
}

/// Probe whether a loopback port speaks SOCKS5 (greeting `05 00`).
pub async fn test_socks5_alive(host: &str, port: u16, timeout_ms: u64) -> bool {
    let Ok(Ok(mut sock)) = timeout(
        Duration::from_millis(timeout_ms),
        TcpStream::connect((host, port)),
    )
    .await
    else {
        return false;
    };
    if sock.write_all(&[0x05, 0x01, 0x00]).await.is_err() {
        return false;
    }
    let mut resp = [0u8; 2];
    if sock.read_exact(&mut resp).await.is_err() {
        return false;
    }
    resp[0] == 5 && resp[1] == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_targets() {
        assert!(matches!(parse_target("127.0.0.1"), Target::V4(_)));
        assert!(matches!(parse_target("::1"), Target::V6(_)));
        assert!(matches!(parse_target("[2001:db8::1]"), Target::V6(_)));
        assert!(matches!(parse_target("example.com"), Target::Domain));
    }

    #[test]
    fn rejects_bad_hosts() {
        assert!(ascii_host("").is_err());
        assert!(ascii_host(&"a".repeat(300)).is_err());
        assert!(ascii_host("münchen.de").is_err());
        assert!(ascii_host("example.com").is_ok());
    }

    #[test]
    fn domain_length_fits_socks5_length_byte() {
        // `ascii_host` caps domains at 253 bytes; the SOCKS5 length byte holds 255.
        assert!(socks_domain(&"a".repeat(253)).is_ok());
        assert!(socks_domain(&"a".repeat(254)).is_err());
    }
}
