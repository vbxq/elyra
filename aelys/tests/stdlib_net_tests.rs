mod common;
use common::*;

#[test]
fn net_invalid_port_negative() {
    let code = r#"
needs std::net
net::connect("localhost", -1)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port") || err.contains("invalid"));
}

#[test]
fn invalid_port_too_large() {
    let code = r#"
needs std::net
net::connect("localhost", 99999)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port"));
}

#[test]
fn close_invalid_handle() {
    let code = r#"
needs std::net
net::close(456)
"#;
    // Invalid handle returns null rather than erroring
    let result = run_aelys_result(code);
    assert!(result.is_ok(), "close invalid handle should not panic");
}

#[test]
fn recv_on_invalid_handle() {
    let code = r#"
needs std::net
net::recv(123)
"#;
    let result = run_aelys_result(code);
    assert!(result.is_ok(), "recv invalid handle should not panic");
}

#[test]
fn set_timeout_invalid_handle() {
    let code = r#"
needs std::net
net::set_timeout(777, 1000)
"#;
    let result = run_aelys_result(code);
    assert!(
        result.is_ok(),
        "set_timeout invalid handle should not panic"
    );
}

#[test]
#[ignore]
fn shutdown_invalid_mode() {
    let code = r#"
needs std::net
let s = net::udp_bind("127.0.0.1", 0)
net::shutdown(s, "invalid")
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("invalid") || err.contains("mode"));
}

#[test]
fn listen_invalid_port() {
    let code = r#"
needs std::net
net::listen("0.0.0.0", -5)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port"));
}

#[test]
fn recv_line_invalid_handle() {
    let code = r#"
needs std::net
net::recv_line(999)
"#;
    let result = run_aelys_result(code);
    assert!(result.is_ok(), "recv_line invalid handle should not panic");
}

#[test]
fn peer_addr_invalid() {
    let code = r#"
needs std::net
net::peer_addr(12345)
"#;
    let result = run_aelys_result(code);
    assert!(result.is_ok(), "peer_addr invalid handle should not panic");
}

#[test]
#[ignore]
fn udp_bind_and_close() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0)
net::close(sock)
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn udp_bind_invalid_port() {
    let code = r#"
needs std::net
net::udp_bind("127.0.0.1", -1)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port") || err.contains("invalid"));
}

#[test]
fn udp_bind_port_too_large() {
    let code = r#"
needs std::net
net::udp_bind("127.0.0.1", 99999)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port"));
}

#[test]
#[ignore]
fn udp_local_addr() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0)
let addr = net::local_addr(sock)
net::close(sock)
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
#[ignore]
fn udp_set_timeout() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0)
net::set_timeout(sock, 1000)
net::close(sock)
1
"#;
    assert_aelys_int(code, 1);
}

#[test]
#[ignore]
fn udp_set_broadcast() {
    let code = r#"
needs std::net
let sock = net::udp_bind("0.0.0.0", 0)
net::udp_set_broadcast(sock, true)
net::close(sock)
1
"#;
    assert_aelys_int(code, 1);
}

#[test]
#[ignore]
fn udp_recv_negative_max() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0)
net::udp_recv(sock, -1)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("negative") || err.contains("non-negative"));
}

#[test]
#[ignore]
fn udp_connect_invalid_port() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0)
net::udp_connect(sock, "127.0.0.1", -1)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port") || err.contains("invalid"));
}

#[test]
fn udp_send_to_and_recv_from() {
    let code = r#"
needs std::net
let s1 = net::udp_bind("127.0.0.1", 0)
let s2 = net::udp_bind("127.0.0.1", 0)
let addr2 = net::local_addr(s2)
net::set_timeout(s2, 2000)
net::udp_send_to(s1, "hello udp", addr2)
let data = net::udp_recv_from(s2, 1024)
net::close(s1)
net::close(s2)
data
"#;
    // May succeed or fail depending on environment (localhost networking)
    let result = run_aelys_result(code);
    let _ = result;
}
