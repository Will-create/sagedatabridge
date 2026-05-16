use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::{TcpStream, UdpSocket};
use tokio::task::JoinSet;
use tokio::time::timeout;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlServerInstance {
    pub host: String,
    pub instance_name: String,
    pub port: u16,
    pub version: String,
    pub method: String,
}

/// Send UDP broadcast to 255.255.255.255:1434 and parse responses
async fn scan_udp_broadcast() -> Vec<SqlServerInstance> {
    let mut instances = Vec::new();
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(_) => return instances,
    };
    
    if let Err(_) = socket.set_broadcast(true) {
        return instances;
    }

    let broadcast_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)), 1434);
    if let Err(_) = socket.send_to(&[0x02], broadcast_addr).await {
        return instances;
    }

    let mut buf = [0u8; 4096];
    let start = std::time::Instant::now();
    let duration = Duration::from_secs(3);

    while start.elapsed() < duration {
        let remaining = duration.saturating_sub(start.elapsed());
        match timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, addr))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                // Format: ServerName;NAME;InstanceName;INST;IsClustered;No;Version;15.0.2000.5;tcp;1433;;
                let parts: Vec<&str> = response.split(';').collect();
                let mut host = addr.ip().to_string();
                let mut instance_name = String::new();
                let mut port = 1433;
                let mut version = String::new();

                for i in (0..parts.len()).step_by(2) {
                    if i + 1 >= parts.len() { break; }
                    let key = parts[i].to_lowercase();
                    let val = parts[i+1];
                    match key.as_str() {
                        "servername" => host = val.to_string(),
                        "instancename" => instance_name = val.to_string(),
                        "tcp" => port = val.parse().unwrap_or(1433),
                        "version" => version = val.to_string(),
                        _ => {}
                    }
                }
                
                instances.push(SqlServerInstance {
                    host: addr.ip().to_string(), // Use IP for reliability in connection
                    instance_name,
                    port,
                    version: format!("SQL Server {}", version),
                    method: "udp_broadcast".into(),
                });
            }
            _ => break,
        }
    }

    instances
}

/// Detect local IP by "connecting" to a public IP
fn get_local_ip() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ipv4) => Some(ipv4),
        _ => None,
    }
}

/// Probe port 1433 on the entire local /24 subnet
async fn scan_tcp_probe() -> Vec<SqlServerInstance> {
    let mut instances = Vec::new();
    let local_ip = match get_local_ip() {
        Some(ip) => ip,
        None => return instances,
    };

    let octets = local_ip.octets();
    let mut join_set = JoinSet::new();
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(50));

    for i in 1..255 {
        if i == octets[3] { continue; } // Skip self
        let target_ip = Ipv4Addr::new(octets[0], octets[1], octets[2], i);
        let sem = semaphore.clone();
        
        join_set.spawn(async move {
            let _permit = sem.acquire().await.ok();
            let addr = SocketAddr::new(IpAddr::V4(target_ip), 1433);
            match timeout(Duration::from_millis(300), TcpStream::connect(addr)).await {
                Ok(Ok(_)) => Some(target_ip),
                _ => None,
            }
        });
    }

    while let Some(res) = join_set.join_next().await {
        if let Ok(Some(ip)) = res {
            instances.push(SqlServerInstance {
                host: ip.to_string(),
                instance_name: String::new(),
                port: 1433,
                version: "SQL Server (Detected)".into(),
                method: "tcp_probe".into(),
            });
        }
    }

    instances
}

#[tauri::command]
pub async fn scan_network_for_sql_servers() -> Result<Vec<SqlServerInstance>, String> {
    let scan_logic = async {
        // Run both methods in parallel
        let udp_task = scan_udp_broadcast();
        let tcp_task = scan_tcp_probe();
        
        let (udp_results, tcp_results) = tokio::join!(udp_task, tcp_task);
        
        let mut combined = udp_results;
        let seen_ips: HashSet<String> = combined.iter().map(|inst| inst.host.clone()).collect();
        
        for tcp_inst in tcp_results {
            if !seen_ips.contains(&tcp_inst.host) {
                combined.push(tcp_inst);
            }
        }
        
        combined.sort_by(|a, b| a.host.cmp(&b.host));
        combined
    };

    match timeout(Duration::from_secs(8), scan_logic).await {
        Ok(results) => Ok(results),
        Err(_) => Err("Network scan timed out after 8 seconds".into()),
    }
}
