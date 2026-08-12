use crate::stdlib::helpers::{
    get_handle, get_int, get_string, make_string, option_some, result_err, result_ok,
};
use crate::stdlib::{
    Resource, StdModuleExports, TcpStreamResource, UdpSocketResource, register_native,
};
use crate::vm::{VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

const MAX_BUFFER_SIZE: usize = 16 * 1024 * 1024;
const MAX_RECV_SIZE: usize = 16 * 1024 * 1024;

/// Register all net functions in the VM.
pub fn register(vm: &mut VM) -> Result<StdModuleExports, RuntimeError> {
    let mut all_exports = Vec::new();
    let mut native_functions = Vec::new();

    macro_rules! reg_fn {
        ($name:expr, $arity:expr, $func:expr) => {{
            register_native(vm, "net", $name, $arity, $func)?;
            all_exports.push($name.to_string());
            native_functions.push(format!("net::{}", $name));
        }};
    }
    reg_fn!("connect", 2, native_connect);
    reg_fn!("connect_timeout", 3, native_connect_timeout);
    reg_fn!("send", 2, native_send);
    reg_fn!("recv", 1, native_recv);
    reg_fn!("recv_bytes", 2, native_recv_bytes);
    reg_fn!("recv_line", 1, native_recv_line);
    reg_fn!("close", 1, native_close);
    reg_fn!("listen", 2, native_listen);
    reg_fn!("accept", 1, native_accept);
    reg_fn!("set_timeout", 2, native_set_timeout);
    reg_fn!("set_nodelay", 2, native_set_nodelay);
    reg_fn!("local_addr", 1, native_local_addr);
    reg_fn!("peer_addr", 1, native_peer_addr);
    reg_fn!("shutdown", 2, native_shutdown);
    reg_fn!("udp_bind", 2, native_udp_bind);
    reg_fn!("udp_send_to", 3, native_udp_send_to);
    reg_fn!("udp_recv_from", 2, native_udp_recv_from);
    reg_fn!("udp_connect", 3, native_udp_connect);
    reg_fn!("udp_send", 2, native_udp_send);
    reg_fn!("udp_recv", 2, native_udp_recv);
    reg_fn!("udp_set_broadcast", 2, native_udp_set_broadcast);

    Ok(StdModuleExports {
        all_exports,
        native_functions,
    })
}

/// Create a network error.
fn net_error(vm: &VM, op: &'static str, msg: String) -> RuntimeError {
    vm.runtime_error(RuntimeErrorKind::TypeError {
        operation: op,
        expected: "valid network operation",
        got: msg,
    })
}

fn net_fail(vm: &mut VM, op: &str, msg: impl AsRef<str>) -> Result<Value, RuntimeError> {
    result_err(vm, &format!("{op}: {}", msg.as_ref()))
}

fn net_ok(vm: &mut VM) -> Result<Value, RuntimeError> {
    result_ok(vm, Value::unit())
}

/// connect(host, port) - Connect to a TCP server.
/// Returns a socket handle.
fn native_connect(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let host = get_string(vm, args[0], "net.connect")?;
    let port = get_int(vm, args[1], "net.connect")?;

    if !(0..=65535).contains(&port) {
        return Err(net_error(
            vm,
            "net.connect",
            format!("invalid port number: {}", port),
        ));
    }

    let addr = format!("{}:{}", host, port);

    // Resolve address
    let addrs: Vec<_> = match addr.to_socket_addrs() {
        Ok(iter) => iter.collect(),
        Err(_) => return Ok(Value::none()),
    };

    if addrs.is_empty() {
        return Ok(Value::none());
    }

    // Try to connect with timeout
    let stream = match TcpStream::connect_timeout(&addrs[0], Duration::from_secs(30)) {
        Ok(s) => s,
        Err(_) => return Ok(Value::none()),
    };

    let resource = TcpStreamResource {
        stream,
        timeout_ms: None,
    };

    let handle = vm.store_resource(Resource::TcpStream(resource));
    option_some(vm, Value::int(handle as i64))
}

/// udp_bind(host,port) Bind an UDP socket
fn native_udp_bind(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let host = get_string(vm, args[0], "net.udp_bind")?.to_string();
    let port = get_int(vm, args[1], "net.udp_bind")?;

    if !(0..=65535).contains(&port) {
        return Err(net_error(
            vm,
            "net.udp_bind",
            format!("invalid port number: {}", port),
        ));
    }

    let addr = format!("{}:{}", host, port);

    let socket = match UdpSocket::bind(&addr) {
        Ok(s) => s,
        Err(_) => return Ok(Value::none()),
    };

    let handle = vm.store_resource(Resource::UdpSocket(UdpSocketResource {
        socket,
        timeout_ms: None,
    }));
    option_some(vm, Value::int(handle as i64))
}

/// udp_send_to(handle, data, addr) - send a data to host:port
/// Returns number of bytes sent
fn native_udp_send_to(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    //this function is for sending some data to udp sockett
    let handle = get_handle(vm, args[0], "net.udp_send_to")?;
    let data = get_string(vm, args[1], "net.udp_send_to")?.to_string();
    let addr = get_string(vm, args[2], "net.udp_send_to")?.to_string();

    let sent = if let Some(Resource::UdpSocket(res)) = vm.get_resource(handle) {
        match res.socket.send_to(data.as_bytes(), &addr) {
            Ok(n) => Some(n as i64),
            Err(_) => None,
        }
    } else {
        None
    };
    match sent {
        Some(value) => option_some(vm, Value::int(value)),
        None => Ok(Value::none()),
    }
}

/// udp_recv_from(handle, max), receive some data
/// returns received data as string
fn native_udp_recv_from(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.udp_recv_from")?;
    let max = get_int(vm, args[1], "net.udp_recv_from")?;

    if max < 0 {
        return Err(net_error(
            vm,
            "net.udp_recv_from",
            "max must be non-negative".to_string(),
        ));
    }

    if max as usize > MAX_BUFFER_SIZE {
        return Err(net_error(
            vm,
            "net.udp_recv_from",
            format!(
                "max exceeds maximum buffer size of {} bytes",
                MAX_BUFFER_SIZE
            ),
        ));
    }

    let received = if let Some(Resource::UdpSocket(res)) = vm.get_resource(handle) {
        let mut buffer = vec![0u8; max as usize];
        match res.socket.recv_from(&mut buffer) {
            Ok((n, _addr)) => {
                buffer.truncate(n);
                Some(String::from_utf8_lossy(&buffer).into_owned())
            }
            Err(_) => None,
        }
    } else {
        None
    };
    match received {
        Some(value) => {
            let value = make_string(vm, &value)?;
            option_some(vm, value)
        }
        None => Ok(Value::none()),
    }
}

/// udp_connect is a function that connects to an UDP server
fn native_udp_connect(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.udp_connect")?;
    let host = get_string(vm, args[1], "net.udp_connect")?.to_string();
    let port = get_int(vm, args[2], "net.udp_connect")?;

    if !(0..=65535).contains(&port) {
        return net_fail(
            vm,
            "net.udp_connect",
            format!("invalid port number: {}", port),
        );
    }

    let addr = format!("{}:{}", host, port);

    let result = match vm.get_resource(handle) {
        Some(Resource::UdpSocket(res)) => res.socket.connect(&addr).map_err(|e| e.to_string()),
        _ => Err("invalid UDP socket handle".to_string()),
    };

    match result {
        Ok(()) => net_ok(vm),
        Err(error) => net_fail(vm, "net.udp_connect", error),
    }
}

/// udp_send(handle, data) - send a data on a udp socket
/// Returns number of bytes sent.
fn native_udp_send(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    //this function is for sending some data to udp sockett
    let handle = get_handle(vm, args[0], "net.udp_send")?;
    let data = get_string(vm, args[1], "net.udp_send")?.to_string();

    let sent = if let Some(Resource::UdpSocket(res)) = vm.get_resource(handle) {
        match res.socket.send(data.as_bytes()) {
            Ok(n) => Some(n as i64),
            Err(_) => None,
        }
    } else {
        None
    };
    match sent {
        Some(value) => option_some(vm, Value::int(value)),
        None => Ok(Value::none()),
    }
}

/// udp_recv_from(handle, max), receive some data on a connected socket
/// returns received data as string
fn native_udp_recv(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.udp_recv")?;
    let max = get_int(vm, args[1], "net.udp_recv")?;

    if max < 0 {
        return Err(net_error(
            vm,
            "net.udp_recv_from",
            "max must be non-negative".to_string(),
        ));
    }

    if max as usize > MAX_BUFFER_SIZE {
        return Err(net_error(
            vm,
            "net.udp_recv",
            format!(
                "max exceeds maximum buffer size of {} bytes",
                MAX_BUFFER_SIZE
            ),
        ));
    }

    let received = if let Some(Resource::UdpSocket(res)) = vm.get_resource(handle) {
        let mut buffer = vec![0u8; max as usize];
        match res.socket.recv(&mut buffer) {
            Ok(n) => {
                buffer.truncate(n);
                Some(String::from_utf8_lossy(&buffer).into_owned())
            }
            Err(_) => None,
        }
    } else {
        None
    };
    match received {
        Some(value) => {
            let value = make_string(vm, &value)?;
            option_some(vm, value)
        }
        None => Ok(Value::none()),
    }
}

/// udp_set_broadcast(handle, enable)[bool] - enable/disable broadcast on a UDP socket
fn native_udp_set_broadcast(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.udp_set_broadcast")?;
    let enabled = args[1].is_truthy();

    let result = match vm.get_resource(handle) {
        Some(Resource::UdpSocket(res)) => {
            res.socket.set_broadcast(enabled).map_err(|e| e.to_string())
        }
        _ => Err("invalid UDP socket handle".to_string()),
    };

    match result {
        Ok(()) => net_ok(vm),
        Err(error) => net_fail(vm, "net.udp_set_broadcast", error),
    }
}
// connect_timeout(host, port, ms) - Connect to a TCP server with a custom timeout in milliseconds.
/// Returns a socket handle, or null on failure.
fn native_connect_timeout(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let host = get_string(vm, args[0], "net.connect_timeout")?;
    let port = get_int(vm, args[1], "net.connect_timeout")?;
    let timeout_ms = get_int(vm, args[2], "net.connect_timeout")?;

    if !(0..=65535).contains(&port) {
        return Err(net_error(
            vm,
            "net.connect_timeout",
            format!("invalid port number: {}", port),
        ));
    }

    if timeout_ms <= 0 {
        return Err(net_error(
            vm,
            "net.connect_timeout",
            "timeout must be positive".to_string(),
        ));
    }

    let addr = format!("{}:{}", host, port);

    let addrs: Vec<_> = match addr.to_socket_addrs() {
        Ok(iter) => iter.collect(),
        Err(_) => return Ok(Value::none()),
    };

    if addrs.is_empty() {
        return Ok(Value::none());
    }

    let stream =
        match TcpStream::connect_timeout(&addrs[0], Duration::from_millis(timeout_ms as u64)) {
            Ok(s) => s,
            Err(_) => return Ok(Value::none()),
        };

    let resource = TcpStreamResource {
        stream,
        timeout_ms: None,
    };

    let handle = vm.store_resource(Resource::TcpStream(resource));
    option_some(vm, Value::int(handle as i64))
}

/// send(handle, data) - Send data over connection.
/// Returns number of bytes sent.
fn native_send(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.send")?;
    let data = get_string(vm, args[1], "net.send")?.to_string();

    let sent = if let Some(Resource::TcpStream(res)) = vm.get_resource_mut(handle) {
        match res.stream.write_all(data.as_bytes()) {
            Ok(_) => {
                let _ = res.stream.flush();
                Some(data.len() as i64)
            }
            Err(_) => None,
        }
    } else {
        None
    };
    match sent {
        Some(value) => option_some(vm, Value::int(value)),
        None => Ok(Value::none()),
    }
}

/// recv(handle) - Receive all available data from connection.
/// Returns received data as string.
fn native_recv(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.recv")?;

    let received = if let Some(Resource::TcpStream(res)) = vm.get_resource_mut(handle) {
        let mut buffer = vec![0u8; 65536];

        if res.timeout_ms.is_none() {
            let _ = res
                .stream
                .set_read_timeout(Some(Duration::from_millis(100)));
        }

        let mut all_data = Vec::new();
        loop {
            match res.stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if all_data.len() + n > MAX_RECV_SIZE {
                        break;
                    }
                    all_data.extend_from_slice(&buffer[..n]);
                    if n < buffer.len() {
                        break;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(_) => break,
            }
        }

        if res.timeout_ms.is_none() {
            let _ = res.stream.set_read_timeout(None);
        }

        Some(String::from_utf8_lossy(&all_data).into_owned())
    } else {
        None
    };
    match received {
        Some(value) => {
            let value = make_string(vm, &value)?;
            option_some(vm, value)
        }
        None => Ok(Value::none()),
    }
}

/// recv_bytes(handle, max) - Receive up to max bytes.
fn native_recv_bytes(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.recv_bytes")?;
    let max = get_int(vm, args[1], "net.recv_bytes")?;

    if max < 0 {
        return Err(net_error(
            vm,
            "net.recv_bytes",
            "max must be non-negative".to_string(),
        ));
    }

    if max as usize > MAX_BUFFER_SIZE {
        return Err(net_error(
            vm,
            "net.recv_bytes",
            format!(
                "max exceeds maximum buffer size of {} bytes",
                MAX_BUFFER_SIZE
            ),
        ));
    }

    let received = if let Some(Resource::TcpStream(res)) = vm.get_resource_mut(handle) {
        let mut buffer = vec![0u8; max as usize];
        match res.stream.read(&mut buffer) {
            Ok(n) => {
                buffer.truncate(n);
                Some(String::from_utf8_lossy(&buffer).into_owned())
            }
            Err(_) => None,
        }
    } else {
        None
    };
    match received {
        Some(value) => {
            let value = make_string(vm, &value)?;
            option_some(vm, value)
        }
        None => Ok(Value::none()),
    }
}

/// recv_line(handle) - Receive a single line (up to newline).
fn native_recv_line(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.recv_line")?;

    let received = if let Some(Resource::TcpStream(res)) = vm.get_resource_mut(handle) {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];

        loop {
            match res.stream.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    if byte[0] == b'\n' {
                        break;
                    }
                    line.push(byte[0]);
                }
                Err(_) => break,
            }
        }

        if line.last() == Some(&b'\r') {
            line.pop();
        }

        Some(String::from_utf8_lossy(&line).into_owned())
    } else {
        None
    };
    match received {
        Some(value) => {
            let value = make_string(vm, &value)?;
            option_some(vm, value)
        }
        None => Ok(Value::none()),
    }
}

/// close(handle) - Close a socket or listener.
fn native_close(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.close")?;

    match vm.take_resource(handle) {
        Some(Resource::TcpStream(res)) => match res.stream.shutdown(Shutdown::Both) {
            Ok(()) => net_ok(vm),
            Err(error) => net_fail(vm, "net.close", error.to_string()),
        },
        Some(Resource::TcpListener(_)) | Some(Resource::UdpSocket(_)) => net_ok(vm),
        _ => net_fail(vm, "net.close", "invalid network handle"),
    }
}

/// listen(host, port) - Start listening for connections.
/// Returns a listener handle.
fn native_listen(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let host = get_string(vm, args[0], "net.listen")?;
    let port = get_int(vm, args[1], "net.listen")?;

    if !(0..=65535).contains(&port) {
        return Err(net_error(
            vm,
            "net.listen",
            format!("invalid port number: {}", port),
        ));
    }

    let addr = format!("{}:{}", host, port);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(_) => return Ok(Value::none()),
    };

    let handle = vm.store_resource(Resource::TcpListener(listener));
    option_some(vm, Value::int(handle as i64))
}

/// accept(handle) - Accept an incoming connection.
/// Returns a socket handle for the new connection.
fn native_accept(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.accept")?;

    // We need to get the listener, accept, then store the new stream
    let stream = if let Some(Resource::TcpListener(listener)) = vm.get_resource(handle) {
        match listener.accept() {
            Ok(s) => s,
            Err(_) => return Ok(Value::none()),
        }
    } else {
        return Ok(Value::none());
    };

    let resource = TcpStreamResource {
        stream: stream.0,
        timeout_ms: None,
    };

    let new_handle = vm.store_resource(Resource::TcpStream(resource));
    option_some(vm, Value::int(new_handle as i64))
}

/// set_timeout(handle, ms) - Set read/write timeout in milliseconds.
/// Use 0 to disable timeout.
fn native_set_timeout(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.set_timeout")?;
    let ms = get_int(vm, args[1], "net.set_timeout")?;

    if ms < 0 {
        return Err(net_error(
            vm,
            "net.set_timeout",
            "timeout must be non-negative".to_string(),
        ));
    }

    let timeout = if ms == 0 {
        None
    } else {
        Some(Duration::from_millis(ms as u64))
    };
    let result = match vm.get_resource_mut(handle) {
        Some(Resource::TcpStream(res)) => {
            let result = res
                .stream
                .set_read_timeout(timeout)
                .and_then(|()| res.stream.set_write_timeout(timeout));
            if result.is_ok() {
                res.timeout_ms = if ms == 0 { None } else { Some(ms as u64) };
            }
            result.map_err(|error| error.to_string())
        }
        Some(Resource::UdpSocket(res)) => {
            let result = res
                .socket
                .set_read_timeout(timeout)
                .and_then(|()| res.socket.set_write_timeout(timeout));
            if result.is_ok() {
                res.timeout_ms = if ms == 0 { None } else { Some(ms as u64) };
            }
            result.map_err(|error| error.to_string())
        }
        _ => Err("invalid network handle".to_string()),
    };
    match result {
        Ok(()) => net_ok(vm),
        Err(error) => net_fail(vm, "net.set_timeout", error),
    }
}

/// set_nodelay(handle, enabled) - Enable/disable Nagle's algorithm.
fn native_set_nodelay(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.set_nodelay")?;
    let enabled = args[1].is_truthy();

    let result = match vm.get_resource_mut(handle) {
        Some(Resource::TcpStream(res)) => res.stream.set_nodelay(enabled),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "invalid network handle",
        )),
    };
    match result {
        Ok(()) => net_ok(vm),
        Err(error) => net_fail(vm, "net.set_nodelay", error.to_string()),
    }
}

/// local_addr(handle) - Get local address as "host:port".
fn native_local_addr(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.local_addr")?;

    let addr = match vm.get_resource(handle) {
        Some(Resource::TcpStream(res)) => res.stream.local_addr(),
        Some(Resource::TcpListener(listener)) => listener.local_addr(),
        Some(Resource::UdpSocket(res)) => res.socket.local_addr(),
        _ => return Ok(Value::none()),
    };

    match addr {
        Ok(a) => {
            let value = make_string(vm, &a.to_string())?;
            option_some(vm, value)
        }
        Err(_) => Ok(Value::none()),
    }
}

/// peer_addr(handle) - Get peer address as "host:port".
fn native_peer_addr(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.peer_addr")?;

    let addr = match vm.get_resource(handle) {
        Some(Resource::TcpStream(res)) => res.stream.peer_addr(),
        _ => return Ok(Value::none()),
    };

    match addr {
        Ok(a) => {
            let value = make_string(vm, &a.to_string())?;
            option_some(vm, value)
        }
        Err(_) => Ok(Value::none()),
    }
}

/// shutdown(handle, how) - Shutdown part of a connection.
/// how: "read", "write", or "both"
fn native_shutdown(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let handle = get_handle(vm, args[0], "net.shutdown")?;
    let how_str = get_string(vm, args[1], "net.shutdown")?;

    let how = match how_str {
        "read" => Shutdown::Read,
        "write" => Shutdown::Write,
        "both" => Shutdown::Both,
        _ => {
            return net_fail(
                vm,
                "net.shutdown",
                format!(
                    "invalid shutdown mode '{}', use 'read', 'write', or 'both'",
                    how_str
                ),
            );
        }
    };

    let result = match vm.get_resource(handle) {
        Some(Resource::TcpStream(res)) => res.stream.shutdown(how),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "invalid network handle",
        )),
    };
    match result {
        Ok(()) => net_ok(vm),
        Err(error) => net_fail(vm, "net.shutdown", error.to_string()),
    }
}
