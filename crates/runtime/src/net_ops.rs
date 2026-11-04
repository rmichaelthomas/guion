//! TCP and UDP socket operations behind `Op::Net`.
//!
//! Sockets are integer handles in a per-instance table. Every call is bounded: connects, accepts,
//! reads and writes take a timeout capped by the host policy, reads are capped in size, and the
//! table holds a fixed number of sockets. Connecting, sending datagrams and binding are checked
//! against exact `host:port` grants (`*` matches any port); nothing is allowed by default.
use super::{HostPolicy, RuntimeError, Shared, SharedText, Span, Value};
use guion_model::{BigInt, NetFunction, TlsFunction};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{
    ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned,
};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_OPEN_SOCKETS: usize = 64;

#[derive(Debug)]
enum Socket {
    Listener(TcpListener),
    Stream(TcpStream),
    Datagram(UdpSocket),
    Tls(TlsStream),
}

enum TlsStream {
    Client(Box<StreamOwned<ClientConnection, TcpStream>>),
    Server(Box<StreamOwned<ServerConnection, TcpStream>>),
}

impl std::fmt::Debug for TlsStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TlsStream")
    }
}

impl TlsStream {
    fn tcp(&self) -> &TcpStream {
        match self {
            TlsStream::Client(stream) => &stream.sock,
            TlsStream::Server(stream) => &stream.sock,
        }
    }

    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            TlsStream::Client(stream) => stream.read(buffer),
            TlsStream::Server(stream) => stream.read(buffer),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            TlsStream::Client(stream) => stream.write_all(data).and_then(|()| stream.flush()),
            TlsStream::Server(stream) => stream.write_all(data).and_then(|()| stream.flush()),
        }
    }
}

#[derive(Debug, Default)]
struct TableState {
    sockets: HashMap<i64, Arc<Mutex<Socket>>>,
    next: i64,
}

/// The open sockets of one program. Parallel workers share the table of the program that started
/// them, so a handle opened in one worker can be used in another; each socket has its own lock, so
/// a slow read on one socket never blocks the others. Only the program's own table closes sockets
/// when it ends.
#[derive(Debug)]
pub(crate) struct SocketTable {
    state: Arc<Mutex<TableState>>,
    owner: bool,
}

impl Default for SocketTable {
    fn default() -> Self {
        Self {
            state: Arc::default(),
            owner: true,
        }
    }
}

impl SocketTable {
    /// A view of this table for a parallel worker. It sees the same handles but does not close
    /// them when the worker ends.
    pub(crate) fn share(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            owner: false,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, TableState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn open_count(&self) -> usize {
        self.state().sockets.len()
    }

    pub(crate) fn close_all(&mut self) {
        if self.owner {
            self.state().sockets.clear();
        }
    }

    fn insert(&self, socket: Socket) -> Result<i64, Failure> {
        let mut state = self.state();
        if state.sockets.len() >= MAX_OPEN_SOCKETS {
            return Err(Failure::new(
                "E_NET_LIMIT",
                format!("at most {MAX_OPEN_SOCKETS} sockets may be open at once"),
            ));
        }
        state.next += 1;
        let handle = state.next;
        state.sockets.insert(handle, Arc::new(Mutex::new(socket)));
        Ok(handle)
    }

    fn get(&self, handle: i64) -> Option<Arc<Mutex<Socket>>> {
        self.state().sockets.get(&handle).cloned()
    }

    fn remove(&self, handle: i64) -> bool {
        self.state().sockets.remove(&handle).is_some()
    }
}

fn locked(slot: &Arc<Mutex<Socket>>) -> std::sync::MutexGuard<'_, Socket> {
    slot.lock().unwrap_or_else(|error| error.into_inner())
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
            if table.remove(handle.to_i64().unwrap_or(-1)) {
                Ok(Value::Unit)
            } else {
                Err(Failure::new("E_NET_HANDLE", "socket handle is not open"))
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
    let accepted = accept_stream(policy, table, handle, timeout)?;
    Ok(int(table.insert(Socket::Stream(accepted))?))
}

fn accept_stream(
    policy: &HostPolicy,
    table: &mut SocketTable,
    handle: i64,
    timeout: i64,
) -> Result<TcpStream, Failure> {
    let timeout = timeout_of(policy, timeout)?;
    let accepted = {
        // Wait on a duplicate of the listener so other workers can accept on it at the same time.
        let listener = {
            let slot = table
                .get(handle)
                .ok_or_else(|| Failure::new("E_NET_HANDLE", "handle is not a listening socket"))?;
            let guard = locked(&slot);
            let Socket::Listener(listener) = &*guard else {
                return Err(Failure::new(
                    "E_NET_HANDLE",
                    "handle is not a listening socket",
                ));
            };
            listener
                .try_clone()
                .map_err(|error| Failure::io(&error, "cannot configure listener"))?
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
    Ok(accepted)
}

fn local_port(table: &SocketTable, handle: i64) -> Result<Value, Failure> {
    let slot = table
        .get(handle)
        .ok_or_else(|| Failure::new("E_NET_HANDLE", "socket handle is not open"))?;
    let address = match &*locked(&slot) {
        Socket::Listener(socket) => socket.local_addr(),
        Socket::Stream(socket) => socket.local_addr(),
        Socket::Datagram(socket) => socket.local_addr(),
        Socket::Tls(stream) => stream.tcp().local_addr(),
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
    let slot = table.get(handle).ok_or_else(|| {
        Failure::new(
            "E_NET_HANDLE",
            "handle is not a connected TCP or TLS socket",
        )
    })?;
    match &mut *locked(&slot) {
        Socket::Stream(stream) => {
            stream
                .set_write_timeout(Some(policy.max_socket_timeout))
                .map_err(|error| Failure::io(&error, "cannot configure socket"))?;
            stream
                .write_all(data)
                .map_err(|error| Failure::io(&error, "send failed"))?;
        }
        Socket::Tls(stream) => {
            stream
                .tcp()
                .set_write_timeout(Some(policy.max_socket_timeout))
                .map_err(|error| Failure::io(&error, "cannot configure socket"))?;
            stream
                .write_all(data)
                .map_err(|error| Failure::io(&error, "send failed"))?;
        }
        _ => {
            return Err(Failure::new(
                "E_NET_HANDLE",
                "handle is not a connected TCP or TLS socket",
            ))
        }
    }
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
    let mut buffer = vec![0u8; max];
    let slot = table.get(handle).ok_or_else(|| {
        Failure::new(
            "E_NET_HANDLE",
            "handle is not a connected TCP or TLS socket",
        )
    })?;
    let count = match &mut *locked(&slot) {
        Socket::Stream(stream) => {
            stream
                .set_read_timeout(Some(timeout))
                .map_err(|error| Failure::io(&error, "cannot configure socket"))?;
            stream.read(&mut buffer)
        }
        Socket::Tls(stream) => {
            stream
                .tcp()
                .set_read_timeout(Some(timeout))
                .map_err(|error| Failure::io(&error, "cannot configure socket"))?;
            stream.read(&mut buffer)
        }
        _ => {
            return Err(Failure::new(
                "E_NET_HANDLE",
                "handle is not a connected TCP or TLS socket",
            ))
        }
    }
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
    let slot = table
        .get(handle)
        .ok_or_else(|| Failure::new("E_NET_HANDLE", "handle is not a UDP socket"))?;
    let guard = locked(&slot);
    let Socket::Datagram(socket) = &*guard else {
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
    let slot = table
        .get(handle)
        .ok_or_else(|| Failure::new("E_NET_HANDLE", "handle is not a UDP socket"))?;
    let guard = locked(&slot);
    let Socket::Datagram(socket) = &*guard else {
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

const MAX_PEM_BYTES: usize = 256 * 1024;

fn crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn tls_io(error: &std::io::Error, what: &str) -> Failure {
    let tls = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>());
    match tls {
        Some(rustls::Error::InvalidCertificate(reason)) => Failure::new(
            "E_TLS_CERTIFICATE",
            format!("{what}: certificate rejected: {reason:?}"),
        ),
        Some(other) => Failure::new("E_TLS_HANDSHAKE", format!("{what}: {other}")),
        None => {
            let failure = Failure::io(error, what);
            if failure.code == "E_NET_IO" {
                Failure::new("E_TLS_HANDSHAKE", failure.message)
            } else {
                failure
            }
        }
    }
}

fn parse_certificates(pem: &str) -> Result<Vec<CertificateDer<'static>>, Failure> {
    if pem.len() > MAX_PEM_BYTES {
        return Err(Failure::new("E_TLS_ARGUMENT", "PEM text is too large"));
    }
    CertificateDer::pem_slice_iter(pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            Failure::new(
                "E_TLS_ARGUMENT",
                format!("invalid certificate PEM: {error}"),
            )
        })
}

fn complete_handshake<D>(
    connection: &mut rustls::ConnectionCommon<D>,
    tcp: &mut TcpStream,
    timeout: Duration,
) -> Result<(), Failure> {
    tcp.set_read_timeout(Some(timeout))
        .and_then(|()| tcp.set_write_timeout(Some(timeout)))
        .map_err(|error| Failure::io(&error, "cannot configure socket"))?;
    let started = Instant::now();
    while connection.is_handshaking() {
        if started.elapsed() >= timeout {
            return Err(Failure::new("E_NET_TIMEOUT", "TLS handshake timed out"));
        }
        connection
            .complete_io(tcp)
            .map_err(|error| tls_io(&error, "TLS handshake failed"))?;
    }
    Ok(())
}

/// `tls_connect(host, port, timeout_ms, extra_ca_pem)` and
/// `tls_accept(listener, timeout_ms, certificate_chain_pem, private_key_pem)`.
pub(crate) fn run_tls(
    policy: &HostPolicy,
    table: &mut SocketTable,
    function: TlsFunction,
    mut operands: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    operands.reverse();
    let mut next = || operands.pop().ok_or_else(|| type_error(span));
    let outcome = match function {
        TlsFunction::Connect => {
            let (Value::Text(host), Value::Int(port), Value::Int(timeout), Value::Text(ca)) =
                (next()?, next()?, next()?, next()?)
            else {
                return Err(type_error(span));
            };
            tls_connect(
                policy,
                table,
                host.as_ref(),
                port.to_i64().unwrap_or(-1),
                timeout.to_i64().unwrap_or(-1),
                ca.as_ref(),
            )
        }
        TlsFunction::Accept => {
            let (Value::Int(handle), Value::Int(timeout), Value::Text(chain), Value::Text(key)) =
                (next()?, next()?, next()?, next()?)
            else {
                return Err(type_error(span));
            };
            tls_accept(
                policy,
                table,
                handle.to_i64().unwrap_or(-1),
                timeout.to_i64().unwrap_or(-1),
                chain.as_ref(),
                key.as_ref(),
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

fn tls_connect(
    policy: &HostPolicy,
    table: &mut SocketTable,
    host: &str,
    port: i64,
    timeout: i64,
    extra_ca: &str,
) -> Result<Value, Failure> {
    let port = port_of(port)?;
    let timeout_value = timeout_of(policy, timeout)?;
    if !granted(&policy.allowed_connect, host, port) {
        return Err(denied("connect", host, port, "--allow-connect"));
    }
    let server_name = ServerName::try_from(host.to_owned()).map_err(|_| {
        Failure::new(
            "E_TLS_ARGUMENT",
            format!("`{host}` is not a valid server name"),
        )
    })?;
    let mut roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if !extra_ca.is_empty() {
        for certificate in parse_certificates(extra_ca)? {
            roots.add(certificate).map_err(|error| {
                Failure::new("E_TLS_ARGUMENT", format!("bad CA certificate: {error}"))
            })?;
        }
    }
    let config = ClientConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()
        .map_err(|error| Failure::new("E_TLS_HANDSHAKE", error.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let mut connection = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|error| Failure::new("E_TLS_HANDSHAKE", error.to_string()))?;
    let mut last = Failure::new("E_NET_RESOLVE", format!("`{host}` has no addresses"));
    for address in resolve(host, port)? {
        match TcpStream::connect_timeout(&address, timeout_value) {
            Ok(mut tcp) => {
                let _ = tcp.set_nodelay(true);
                complete_handshake(&mut connection, &mut tcp, timeout_value)?;
                let stream = StreamOwned::new(connection, tcp);
                return Ok(int(
                    table.insert(Socket::Tls(TlsStream::Client(Box::new(stream))))?
                ));
            }
            Err(error) => last = Failure::io(&error, &format!("cannot connect to {host}:{port}")),
        }
    }
    Err(last)
}

fn tls_accept(
    policy: &HostPolicy,
    table: &mut SocketTable,
    handle: i64,
    timeout: i64,
    chain: &str,
    key: &str,
) -> Result<Value, Failure> {
    let timeout_value = timeout_of(policy, timeout)?;
    let certificates = parse_certificates(chain)?;
    if key.len() > MAX_PEM_BYTES {
        return Err(Failure::new("E_TLS_ARGUMENT", "PEM text is too large"));
    }
    let key = PrivateKeyDer::from_pem_slice(key.as_bytes()).map_err(|error| {
        Failure::new(
            "E_TLS_ARGUMENT",
            format!("invalid private key PEM: {error}"),
        )
    })?;
    let config = ServerConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()
        .map_err(|error| Failure::new("E_TLS_HANDSHAKE", error.to_string()))?
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map_err(|error| {
            Failure::new(
                "E_TLS_ARGUMENT",
                format!("certificate and key do not match: {error}"),
            )
        })?;
    let mut tcp = accept_stream(policy, table, handle, timeout)?;
    let mut connection = ServerConnection::new(Arc::new(config))
        .map_err(|error| Failure::new("E_TLS_HANDSHAKE", error.to_string()))?;
    complete_handshake(&mut connection, &mut tcp, timeout_value)?;
    let stream = StreamOwned::new(connection, tcp);
    Ok(int(table.insert(Socket::Tls(TlsStream::Server(
        Box::new(stream),
    )))?))
}
