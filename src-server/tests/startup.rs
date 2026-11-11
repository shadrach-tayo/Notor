use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_notor"));
    command
        .env("APP_ENVIRONMENT", "development")
        .env_remove("APP_APPLICATION__GOOGLE_CLIENT_ID")
        .env_remove("APP_APPLICATION__GOOGLE_CLIENT_SECRET")
        .env_remove("APP_APPLICATION__GOOGLE_CALENDAR_API_KEY")
        .env_remove("APP_APPLICATION__GOOGLE_REDIRECT_URL")
        .env_remove("APP_APPLICATION__SERVE_CLIENT_SECRET_TO_LEGACY_CLIENTS");
    command
}

#[test]
fn the_binary_refuses_to_start_and_names_missing_settings() {
    let output = binary()
        .env_remove("APP_APPLICATION__PORT")
        .output()
        .expect("run notor");
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Error: missing or invalid settings:"));
    assert!(stderr
        .contains("application.google_client_id (APP_APPLICATION__GOOGLE_CLIENT_ID) is not set"));
    assert!(stderr.contains(
        "application.google_client_secret (APP_APPLICATION__GOOGLE_CLIENT_SECRET) is not set"
    ));
    assert!(stderr.contains(
        "application.google_redirect_url (APP_APPLICATION__GOOGLE_REDIRECT_URL) is not set"
    ));
}

#[test]
fn the_binary_starts_when_every_setting_is_set() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let child = binary()
        .env("APP_APPLICATION__PORT", port.to_string())
        .env("APP_APPLICATION__GOOGLE_CLIENT_ID", "client-id")
        .env("APP_APPLICATION__GOOGLE_CLIENT_SECRET", "client-secret")
        .env(
            "APP_APPLICATION__GOOGLE_REDIRECT_URL",
            "https://notor.vercel.app/auth",
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn notor");
    let _guard = ChildGuard(child);
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut response = String::new();
    while Instant::now() < deadline {
        if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.write_all(
                b"GET /health-check HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            );
            response.clear();
            let _ = stream.read_to_string(&mut response);
            if response.contains("200") {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server did not answer on {port}: {response}");
}
