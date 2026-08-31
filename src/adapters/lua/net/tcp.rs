use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mlua::{Lua, Result, Table, UserData, UserDataMethods};

use crate::application::context::BindingContext;

struct TcpConnection {
    reader: Mutex<BufReader<TcpStream>>,
    writer: Mutex<TcpStream>,
    addr: String,
}

impl TcpConnection {
    fn new(stream: TcpStream, addr: String, timeout: Duration) -> Self {
        let writer = stream.try_clone().expect("failed to clone tcp stream");
        stream.set_read_timeout(Some(timeout)).ok();
        stream.set_write_timeout(Some(timeout)).ok();
        writer.set_read_timeout(Some(timeout)).ok();
        writer.set_write_timeout(Some(timeout)).ok();
        Self {
            reader: Mutex::new(BufReader::new(stream)),
            writer: Mutex::new(writer),
            addr,
        }
    }
}

impl UserData for TcpConnection {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("send", |_, conn, data: String| {
            let mut writer = conn.writer.lock().expect("poisoned lock");
            writer
                .write_all(data.as_bytes())
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            Ok(())
        });

        methods.add_method("recv", |_, conn, n: usize| {
            let mut reader = conn.reader.lock().expect("poisoned lock");
            let mut buf = vec![0u8; n];
            let read = reader
                .read(&mut buf)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            if read == 0 {
                return Ok(None);
            }
            buf.truncate(read);
            Ok(Some(String::from_utf8_lossy(&buf).to_string()))
        });

        methods.add_method("recv_line", |_, conn, ()| {
            let mut reader = conn.reader.lock().expect("poisoned lock");
            let mut buf = Vec::new();
            let read = reader
                .read_until(b'\n', &mut buf)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            if read == 0 {
                return Ok(None);
            }
            Ok(Some(String::from_utf8_lossy(&buf).to_string()))
        });

        methods.add_method("recv_until", |_, conn, delim: String| {
            let mut reader = conn.reader.lock().expect("poisoned lock");
            let delim_bytes = delim.as_bytes();
            if delim_bytes.is_empty() {
                return Err(mlua::Error::RuntimeError("empty delimiter".into()));
            }

            let mut buf = Vec::new();

            if delim_bytes.len() == 1 {
                let read = reader
                    .read_until(delim_bytes[0], &mut buf)
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                if read == 0 {
                    return Ok(None);
                }
            } else {
                let mut byte = [0u8; 1];
                loop {
                    match reader.read(&mut byte) {
                        Ok(0) => break,
                        Ok(_) => {
                            buf.push(byte[0]);
                            if buf.ends_with(delim_bytes) {
                                break;
                            }
                        }
                        Err(e) => return Err(mlua::Error::RuntimeError(e.to_string())),
                    }
                }
                if buf.is_empty() {
                    return Ok(None);
                }
            }

            Ok(Some(String::from_utf8_lossy(&buf).to_string()))
        });

        methods.add_method("close", |_, conn, ()| {
            let writer = conn.writer.lock().expect("poisoned lock");
            writer
                .shutdown(Shutdown::Both)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            Ok(())
        });

        methods.add_method("addr", |_, conn, ()| Ok(conn.addr.clone()));

        methods.add_method("set_timeout", |_, conn, secs: u64| {
            let timeout = Some(Duration::from_secs(secs));
            let reader = conn.reader.lock().expect("poisoned lock");
            reader
                .get_ref()
                .set_read_timeout(timeout)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            let writer = conn.writer.lock().expect("poisoned lock");
            writer
                .set_write_timeout(timeout)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            Ok(())
        });

        methods.add_method("set_nodelay", |_, conn, enabled: bool| {
            let writer = conn.writer.lock().expect("poisoned lock");
            writer
                .set_nodelay(enabled)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            Ok(())
        });
    }
}

pub fn register(lua: &Lua, dstest: &Table, ctx: &BindingContext) -> Result<()> {
    let state = Arc::clone(ctx.state());

    let tcp_fn = lua.create_function(move |lua, (id, port): (String, u16)| {
        let (host, timeout_secs) = {
            let state = state.lock().expect("poisoned lock");
            let host = state
                .subjects
                .host_for(&id)
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(format!("unknown subject {} (or no address)", id))
                })?
                .to_string();
            let config = state
                .subjects
                .config_for(&id)
                .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown subject {}", id)))?;
            let cfg = state.configs.get(config).ok_or_else(|| {
                mlua::Error::RuntimeError(format!("subject {} has unknown config '{}'", id, config))
            })?;
            (host, cfg.http_timeout_secs)
        };

        let host_ip = host.split(':').next().unwrap_or(&host);
        let addr = format!("{}:{}", host_ip, port);

        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| mlua::Error::RuntimeError(format!("invalid address: {}", e)))?
            .next()
            .ok_or_else(|| mlua::Error::RuntimeError("address resolved to nothing".to_string()))?;

        match TcpStream::connect_timeout(&socket_addr, Duration::from_secs(timeout_secs)) {
            Ok(stream) => {
                let conn = TcpConnection::new(stream, addr, Duration::from_secs(timeout_secs));
                let ud = lua.create_userdata(conn)?;
                Ok((Some(ud), None::<String>))
            }
            Err(e) => Ok((None::<mlua::AnyUserData>, Some(e.to_string()))),
        }
    })?;

    dstest.set("tcp", tcp_fn)?;
    Ok(())
}
