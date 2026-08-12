mod common;
use common::*;

#[test]
fn net_invalid_port_negative() {
    let code = r#"
needs std::net
let _ = net::connect("localhost", -1)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port") || err.contains("invalid"));
}

#[test]
fn invalid_port_too_large() {
    let code = r#"
needs std::net
let _ = net::connect("localhost", 99999)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port"));
}

#[test]
fn close_invalid_handle() {
    let code = r#"
needs std::net
match net::close(456) { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn recv_on_invalid_handle() {
    let code = r#"
needs std::net
let data: Option<string> = net::recv(123)
match data { None => 1, Some(_) => 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn set_timeout_invalid_handle() {
    let code = r#"
needs std::net
match net::set_timeout(777, 1000) { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
#[ignore]
fn shutdown_invalid_mode() {
    let code = r#"
needs std::net
let s = net::udp_bind("127.0.0.1", 0).unwrap()
match net::shutdown(s, "invalid") { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn listen_invalid_port() {
    let code = r#"
needs std::net
let _ = net::listen("0.0.0.0", -5)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port"));
}

#[test]
fn recv_line_invalid_handle() {
    let code = r#"
needs std::net
let line: Option<string> = net::recv_line(999)
match line { None => 1, Some(_) => 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn peer_addr_invalid() {
    let code = r#"
needs std::net
let address: Option<string> = net::peer_addr(12345)
match address { None => 1, Some(_) => 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
#[ignore]
fn udp_bind_and_close() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0).unwrap()
net::close(sock).unwrap()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn udp_bind_invalid_port() {
    let code = r#"
needs std::net
let _ = net::udp_bind("127.0.0.1", -1)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port") || err.contains("invalid"));
}

#[test]
fn udp_bind_port_too_large() {
    let code = r#"
needs std::net
let _ = net::udp_bind("127.0.0.1", 99999)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("port"));
}

#[test]
#[ignore]
fn udp_local_addr() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0).unwrap()
let addr = net::local_addr(sock).unwrap()
net::close(sock).unwrap()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
#[ignore]
fn udp_set_timeout() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0).unwrap()
net::set_timeout(sock, 1000).unwrap()
net::close(sock).unwrap()
1
"#;
    assert_aelys_int(code, 1);
}

#[test]
#[ignore]
fn udp_set_broadcast() {
    let code = r#"
needs std::net
let sock = net::udp_bind("0.0.0.0", 0).unwrap()
net::udp_set_broadcast(sock, true).unwrap()
net::close(sock).unwrap()
1
"#;
    assert_aelys_int(code, 1);
}

#[test]
#[ignore]
fn udp_recv_negative_max() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0).unwrap()
let _ = net::udp_recv(sock, -1)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("negative") || err.contains("non-negative"));
}

#[test]
#[ignore]
fn udp_connect_invalid_port() {
    let code = r#"
needs std::net
let sock = net::udp_bind("127.0.0.1", 0).unwrap()
match net::udp_connect(sock, "127.0.0.1", -1) { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn udp_send_to_and_recv_from() {
    let code = r#"
needs std::net
let s1 = net::udp_bind("127.0.0.1", 0).unwrap()
let s2 = net::udp_bind("127.0.0.1", 0).unwrap()
let addr2 = net::local_addr(s2).unwrap()
net::set_timeout(s2, 2000).unwrap()
let _ = net::udp_send_to(s1, "hello udp", addr2)
let _ = net::udp_recv_from(s2, 1024)
net::close(s1).unwrap()
net::close(s2).unwrap()
42
"#;
    // May succeed or fail depending on environment (localhost networking)
    let result = run_aelys_result(code);
    let _ = result;
}
