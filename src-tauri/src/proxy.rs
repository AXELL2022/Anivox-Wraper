use std::fs;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use fast_socks5::server::Socks5ServerProtocol;
use fast_socks5::{ReplyError, Socks5Command};
use tokio::net::TcpListener;

const WIREPROXY_BIN: &[u8] = include_bytes!("../binaries/wireproxy");

pub struct SmartProxy {
    pub is_vpn_enabled: Arc<AtomicBool>,
    pub port: u16,
    _child: Arc<Mutex<Option<Child>>>,
}

impl Drop for SmartProxy {
    fn drop(&mut self) {
        if let Ok(mut lock) = self._child.lock() {
            if let Some(mut child) = lock.take() {
                let _ = child.kill();
            }
        }
    }
}

impl SmartProxy {
    pub async fn start(wg_config_str: &str) -> Result<Self, String> {
        let temp_dir = std::env::temp_dir();
        let wireproxy_path = temp_dir.join("anivox_wireproxy");
        let conf_path = temp_dir.join("anivox_wireproxy.conf");

        // Overwrite or write binary if missing or size differs
        let needs_write = !wireproxy_path.exists()
            || wireproxy_path.metadata().map(|m| m.len()).unwrap_or(0) != WIREPROXY_BIN.len() as u64;
        if needs_write {
            fs::write(&wireproxy_path, WIREPROXY_BIN)
                .map_err(|e| format!("Failed to write wireproxy binary: {}", e))?;
        }

        // Ensure executable permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = fs::metadata(&wireproxy_path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&wireproxy_path, perms);
            }
        }

        // Parse WG config and format for wireproxy SOCKS5 and HTTP inbound
        let conf_content = format!(
            "{}\n\n[Socks5]\nBindAddress = 127.0.0.1:10809\n\n[Http]\nBindAddress = 127.0.0.1:10807\n",
            wg_config_str.trim()
        );
        fs::write(&conf_path, conf_content)
            .map_err(|e| format!("Failed to write wireproxy config: {}", e))?;

        // Kill any previous orphan wireproxy instance
        let _ = Command::new("pkill")
            .args(["-f", "anivox_wireproxy"])
            .status();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Spawn wireproxy
        let mut cmd = Command::new(&wireproxy_path);
        cmd.args(["-c", conf_path.to_str().unwrap(), "--silent"]);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start wireproxy: {}", e))?;

        let child_arc = Arc::new(Mutex::new(Some(child)));

        // Wait up to 2 seconds for wireproxy ports to be ready
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let s_ok = tokio::net::TcpStream::connect("127.0.0.1:10809").await.is_ok();
            let h_ok = tokio::net::TcpStream::connect("127.0.0.1:10807").await.is_ok();
            if s_ok && h_ok {
                break;
            }
        }

        let listener = match TcpListener::bind("127.0.0.1:10808").await {
            Ok(l) => l,
            Err(_) => TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| format!("Failed to bind router port: {}", e))?,
        };

        let port = listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .port();

        // VPN is ON by default
        let is_vpn_enabled = Arc::new(AtomicBool::new(true));
        let is_vpn_clone = Arc::clone(&is_vpn_enabled);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, _)) => {
                        let vpn_on = is_vpn_clone.load(Ordering::SeqCst);
                        tokio::spawn(async move {
                            if vpn_on {
                                // Fast transparent pipe directly into wireproxy with TCP_NODELAY
                                let _ = socket.set_nodelay(true);
                                match tokio::net::TcpStream::connect("127.0.0.1:10809").await {
                                    Ok(mut upstream) => {
                                        let _ = upstream.set_nodelay(true);
                                        let mut client = socket;
                                        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                                    }
                                    Err(e) => {
                                        eprintln!("[SmartProxy] Error connecting to wireproxy: {:#}", e);
                                    }
                                }
                            } else {
                                // Direct mode (no VPN)
                                let _ = handle_direct_client(socket).await;
                            }
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            is_vpn_enabled,
            port,
            _child: child_arc,
        })
    }
}

async fn handle_direct_client(
    socket: tokio::net::TcpStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (proto, cmd, target_addr) = Socks5ServerProtocol::accept_no_auth(socket)
        .await
        .map_err(|e| format!("SOCKS5 accept error: {}", e))?
        .read_command()
        .await
        .map_err(|e| format!("SOCKS5 read command error: {}", e))?;

    if cmd != Socks5Command::TCPConnect {
        let _ = proto.reply_error(&ReplyError::CommandNotSupported).await;
        return Err(format!("Unsupported command: {:?}", cmd).into());
    }

    let target_str = target_addr.to_string();
    match tokio::net::TcpStream::connect(&target_str).await {
        Ok(mut direct_stream) => {
            let mut client_stream = proto
                .reply_success(std::net::SocketAddr::from(([0, 0, 0, 0], 0)))
                .await
                .map_err(|e| format!("Reply success error: {}", e))?;

            let _ = tokio::io::copy_bidirectional(&mut client_stream, &mut direct_stream).await;
        }
        Err(e) => {
            let _ = proto.reply_error(&ReplyError::HostUnreachable).await;
            return Err(format!("Failed to connect to target {}: {}", target_str, e).into());
        }
    }

    Ok(())
}
