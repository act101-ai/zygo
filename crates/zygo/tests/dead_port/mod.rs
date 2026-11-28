//! A loopback port that refuses every connection and that no other test can get (FX-6, CI run 45557591675): port 1.
//! A `127.0.0.1:0` bind takes a port from the ephemeral range of the OS, and on Linux, macOS and Windows that range
//! holds no port below 1024. No test binds a fixed port. Nothing listens on port 1, so a connect gets
//! `ConnectionRefused`. No socket is held and no port is read and freed.
#![allow(dead_code)] // each test binary uses a part of this module

/// Port 1 of 127.0.0.1, for each call.
pub fn closed_port() -> u16 {
    1
}

/// `http://127.0.0.1:<closed_port>/`.
pub fn closed_url() -> String {
    format!("http://127.0.0.1:{}/", closed_port())
}
