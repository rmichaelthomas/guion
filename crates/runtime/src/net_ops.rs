//! TCP and UDP socket operations behind `Op::Net`.
//!
//! Sockets are integer handles in a per-instance table. Every call is bounded: connects, accepts,
//! reads and writes take a timeout capped by the host policy, reads are capped in size, and the
//! table holds a fixed number of sockets. Connecting, sending datagrams and binding are checked
//! against exact `host:port` grants (`*` matches any port); nothing is allowed by default.
use super::{HostPolicy, RuntimeError, Shared, SharedText, Span, Value};
use guion_model::{BigInt, NetFunction};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

const MAX_OPEN_SOCKETS: usize = 64;

#[derive(Debug)]
enum Socket {
    Listener(TcpListener),
    Stream(TcpStream),
    Datagram(UdpSocket),
}

#[derive(Debug, Default)]
pub(crate) struct SocketTable {
    sockets: HashMap<i64, Socket>,
    next: i64,
}

impl SocketTable {
    pub(crate) fn open_count(&self) -> usize {
        self.sockets.len()
    }

    pub(crate) fn close_all(&mut self) {
        self.sockets.clear();
    }

    fn insert(&mut self, socket: Socket) -> Result<i64, Failure> {
        if self.sockets.len() >= MAX_OPEN_SOCKETS {
            return Err(Failure::new(
                "E_NET_LIMIT",
                format!("at most {MAX_OPEN_SOCKETS} sockets may be open at once"),
            ));
        }
        self.next += 1;
        self.sockets.insert(self.next, socket);
        Ok(self.next)
    }
}

struct Failure {
    code: &'static str,
    message: String,
}

impl Failure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn io(error: &std::io::Error, what: &str) -> Self {
        let code = match error.kind() {
            ErrorKind::WouldBlock | ErrorKind::TimedOut => "E_NET_TIMEOUT",
            ErrorKind::ConnectionRefused => "E_NET_REFUSED",
            _ => "E_NET_IO",
        };
        Self::new(code, format!("{what}: {error}"))
    }
}

fn text(value: &str) -> Value {
    Value::Text(SharedText::from(value))
}

fn int(value: i64) -> Value {
    Value::Int(BigInt::from_i64(value))
}

fn type_error(span: Span) -> RuntimeError {
    RuntimeError::new(
        "R_TYPE_INVALID",
        "socket function received an invalid operand",
        span,
    )
}

fn granted(grants: &std::collections::HashSet<String>, host: &str, port: u16) -> bool {
    grants.contains(&format!("{host}:{port}")) || grants.contains(&format!("{host}:*"))
}

fn port_of(value: i64) -> Result<u16, Failure> {
    u16::try_from(value).map_err(|_| Failure::new("E_NET_ARGUMENT", "port must be 0 to 65535"))
}

fn timeout_of(policy: &HostPolicy, value: i64) -> Result<Duration, Failure> {
    let limit = policy.max_socket_timeout.as_millis() as i64;
    if value < 1 || value > limit {
        return Err(Failure::new(
            "E_NET_ARGUMENT",
            format!("timeout must be 1 to {limit} milliseconds"),
        ));
    }
    Ok(Duration::from_millis(value as u64))
}

fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, Failure> {
    (host, port)
        .to_socket_addrs()
        .map(|addresses| addresses.collect())
        .map_err(|error| Failure::new("E_NET_RESOLVE", format!("cannot resolve `{host}`: {error}")))
}

pub(crate) fn run(
    policy: &HostPolicy,
    table: &mut SocketTable,
    function: NetFunction,
    mut operands: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    // Operands arrive in push order; reverse so `pop` yields the first one.
    operands.reverse();
    let mut next = || operands.pop().ok_or_else(|| type_error(span));
    let outcome = match function {
        NetFunction::TcpConnect => {
            let (Value::Text(host), Value::Int(port), Value::Int(timeout)) =
                (next()?, next()?, next()?)
            else {
                return Err(type_error(span));
            };
            let (port, timeout) = (port.to_i64().unwrap_or(-1), timeout.to_i64().unwrap_or(-1));
            tcp_connect(policy, table, host.as_ref(), port, timeout)
        }
        NetFunction::TcpListen | NetFunction::UdpBind => {
            let (Value::Text(host), Value::Int(port)) = (next()?, next()?) else {
                return Err(type_error(span));
            };
            bind(
                policy,
                table,
                function,
                host.as_ref(),
                port.to_i64().unwrap_or(-1),
            )
        }
        NetFunction::TcpAccept => {
            let (Value::Int(handle), Value::Int(timeout)) = (next()?, next()?) else {
                return Err(type_error(span));
            };
            tcp_accept(
                policy,
                table,
                handle.to_i64().unwrap_or(-1),
                timeout.to_i64().unwrap_or(-1),
            )
        }
        NetFunction::LocalPort => {
            let Value::Int(handle) = next()? else {
                return Err(type_error(span));
            };
            local_port(table, handle.to_i64().unwrap_or(-1))
        }
        NetFunction::Send => {
            let (Value::Int(handle), Value::Bytes(data)) = (next()?, next()?) else {
                return Err(type_error(span));
            };
            send(policy, table, handle.to_i64().unwrap_or(-1), &data)
        }
        NetFunction::Recv => {
            let (Value::Int(handle), Value::Int(max), Value::Int(timeout)) =
                (next()?, next()?, next()?)
            else {
                return Err(type_error(span));
            };
            recv(
                policy,
                table,
                handle.to_i64().unwrap_or(-1),
                max.to_i64().unwrap_or(-1),
                timeout.to_i64().unwrap_or(-1),
            )
        }
        NetFunction::Close => {
            let Value::Int(handle) = next()? else {
                return Err(type_error(span));
            };
            match table.sockets.remove(&handle.to_i64().unwrap_or(-1)) {
                Some(_) => Ok(Value::Unit),
                None => Err(Failure::new("E_NET_HANDLE", "socket handle is not open")),
            }
        }
        NetFunction::UdpSendTo => {
            let (Value::Int(handle), Value::Text(host), Value::Int(port), Value::Bytes(data)) =
                (next()?, next()?, next()?, next()?)
            else {
                return Err(type_error(span));
            };
            udp_send_to(
                policy,
                table,
                handle.to_i64().unwrap_or(-1),
                host.as_ref(),
                port.to_i64().unwrap_or(-1),
                &data,
            )
        }
        NetFunction::UdpRecvFrom => {
            let (Value::Int(handle), Value::Int(max), Value::Int(timeout)) =
                (next()?, next()?, next()?)
            else {
                return Err(type_error(span));
            };
            udp_recv_from(
                policy,
                table,
                handle.to_i64().unwrap_or(-1),
                max.to_i64().unwrap_or(-1),
                timeout.to_i64().unwrap_or(-1),
            )
        }
    };
    Ok(match outcome {
        Ok(value) => Value::ResultOk(Box::new(value)),
        Err(failure) => Value::ResultErr(Box::new(Value::List(Shared::new(vec![
            text(failure.code),
            text(&failure.message),
        ])))),
    })
}

fn tcp_connect(
    policy: &HostPolicy,
    table: &mut SocketTable,
    host: &str,
    port: i64,
    timeout: i64,
) -> Result<Value, Failure> {
    let port = port_of(port)?;
    let timeout = timeout_of(policy, timeout)?;
    if !granted(&policy.allowed_connect, host, port) {
        return Err(denied("connect", host, port, "--allow-connect"));
    }
    let mut last = Failure::new("E_NET_RESOLVE", format!("`{host}` has no addresses"));
    for address in resolve(host, port)? {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok(int(table.insert(Socket::Stream(stream))?));
            }
            Err(error) => last = Failure::io(&error, &format!("cannot connect to {host}:{port}")),
        }
    }
    Err(last)
}

fn denied(action: &str, host: &str, port: u16, flag: &str) -> Failure {
    Failure::new(
        "E_NET_DENIED",
        format!(
            "{action} to `{host}:{port}` is not granted; add it to guion.toml [sockets] or pass {flag} HOST:PORT"
        ),
    )
}

fn bind(
    policy: &HostPolicy,
    table: &mut SocketTable,
    function: NetFunction,
    host: &str,
    port: i64,
) -> Result<Value, Failure> {
    let port = port_of(port)?;
    if !granted(&policy.allowed_bind, host, port) {
        return Err(denied("binding", host, port, "--allow-bind"));
    }
    let address = resolve(host, port)?
        .into_iter()
        .next()
        .ok_or_else(|| Failure::new("E_NET_RESOLVE", format!("`{host}` has no addresses")))?;
    let socket = if function == NetFunction::TcpListen {
        Socket::Listener(
            TcpListener::bind(address)
                .map_err(|error| Failure::io(&error, &format!("cannot listen on {host}:{port}")))?,
        )
    } else {
        Socket::Datagram(
            UdpSocket::bind(address)
                .map_err(|error| Failure::io(&error, &format!("cannot bind {host}:{port}")))?,
        )
    };
    Ok(int(table.insert(socket)?))
}

fn tcp_accept(
    policy: &HostPolicy,
    table: &mut SocketTable,
    handle: i64,
    timeout: i64,
) -> Result<Value, Failure> {
    let timeout = timeout_of(policy, timeout)?;
    let accepted = {
        let Some(Socket::Listener(listener)) = table.sockets.get(&handle) else {
            return Err(Failure::new(
                "E_NET_HANDLE",
                "handle is not a listening socket",
            ));
        };
        listener
            .set_nonblocking(true)
            .map_err(|error| Failure::io(&error, "cannot configure listener"))?;
        let started = Instant::now();
        loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if started.elapsed() >= timeout {
                        return Err(Failure::new(
                            "E_NET_TIMEOUT",
                            "no connection arrived in time",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => return Err(Failure::io(&error, "accept failed")),
            }
        }
    };
    accepted
        .set_nonblocking(false)
        .map_err(|error| Failure::io(&error, "cannot configure connection"))?;
    let _ = accepted.set_nodelay(true);
    Ok(int(table.insert(Socket::Stream(accepted))?))
}

fn local_port(table: &SocketTable, handle: i64) -> Result<Value, Failure> {
    let address = match table.sockets.get(&handle) {
        Some(Socket::Listener(socket)) => socket.local_addr(),
        Some(Socket::Stream(socket)) => socket.local_addr(),
        Some(Socket::Datagram(socket)) => socket.local_addr(),
        None => return Err(Failure::new("E_NET_HANDLE", "socket handle is not open")),
    }
    .map_err(|error| Failure::io(&error, "cannot read local address"))?;
    Ok(int(i64::from(address.port())))
}

fn send(
    policy: &HostPolicy,
    table: &mut SocketTable,
    handle: i64,
    data: &[u8],
) -> Result<Value, Failure> {
    let Some(Socket::Stream(stream)) = table.sockets.get_mut(&handle) else {
        return Err(Failure::new(
            "E_NET_HANDLE",
            "handle is not a connected TCP socket",
        ));
    };
    stream
        .set_write_timeout(Some(policy.max_socket_timeout))
        .map_err(|error| Failure::io(&error, "cannot configure socket"))?;
    stream
        .write_all(data)
        .map_err(|error| Failure::io(&error, "send failed"))?;
    Ok(int(data.len() as i64))
}

fn read_limit(policy: &HostPolicy, max: i64) -> Result<usize, Failure> {
    let cap = policy.max_socket_read_bytes as i64;
    if max < 1 || max > cap {
        return Err(Failure::new(
            "E_NET_ARGUMENT",
            format!("read size must be 1 to {cap} bytes"),
        ));
    }
    Ok(max as usize)
}

fn recv(
    policy: &HostPolicy,
    table: &mut SocketTable,
    handle: i64,
    max: i64,
    timeout: i64,
) -> Result<Value, Failure> {
    let max = read_limit(policy, max)?;
    let timeout = timeout_of(policy, timeout)?;
    let Some(Socket::Stream(stream)) = table.sockets.get_mut(&handle) else {
        return Err(Failure::new(
            "E_NET_HANDLE",
            "handle is not a connected TCP socket",
        ));
    };
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| Failure::io(&error, "cannot configure socket"))?;
    let mut buffer = vec![0u8; max];
    let count = stream
        .read(&mut buffer)
        .map_err(|error| Failure::io(&error, "receive failed"))?;
    buffer.truncate(count);
    Ok(Value::Bytes(Shared::new(buffer)))
}

fn udp_send_to(
    policy: &HostPolicy,
    table: &mut SocketTable,
    handle: i64,
    host: &str,
    port: i64,
    data: &[u8],
) -> Result<Value, Failure> {
    let port = port_of(port)?;
    if !granted(&policy.allowed_connect, host, port) {
        return Err(denied("sending", host, port, "--allow-connect"));
    }
    if data.len() > policy.max_socket_read_bytes {
        return Err(Failure::new("E_NET_ARGUMENT", "datagram is too large"));
    }
    let address = resolve(host, port)?
        .into_iter()
        .next()
        .ok_or_else(|| Failure::new("E_NET_RESOLVE", format!("`{host}` has no addresses")))?;
    let Some(Socket::Datagram(socket)) = table.sockets.get(&handle) else {
        return Err(Failure::new("E_NET_HANDLE", "handle is not a UDP socket"));
    };
    let sent = socket
        .send_to(data, address)
        .map_err(|error| Failure::io(&error, "send failed"))?;
    Ok(int(sent as i64))
}

fn udp_recv_from(
    policy: &HostPolicy,
    table: &mut SocketTable,
    handle: i64,
    max: i64,
    timeout: i64,
) -> Result<Value, Failure> {
    let max = read_limit(policy, max)?;
    let timeout = timeout_of(policy, timeout)?;
    let Some(Socket::Datagram(socket)) = table.sockets.get(&handle) else {
        return Err(Failure::new("E_NET_HANDLE", "handle is not a UDP socket"));
    };
    socket
        .set_read_timeout(Some(timeout))
        .map_err(|error| Failure::io(&error, "cannot configure socket"))?;
    let mut buffer = vec![0u8; max];
    let (count, sender) = socket
        .recv_from(&mut buffer)
        .map_err(|error| Failure::io(&error, "receive failed"))?;
    buffer.truncate(count);
    Ok(Value::List(Shared::new(vec![
        Value::Bytes(Shared::new(buffer)),
        Value::Bytes(Shared::new(sender.to_string().into_bytes())),
    ])))
}
