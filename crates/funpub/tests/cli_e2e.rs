use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;

use funsecret_core::SecretManage;

fn read_request(stream: &mut TcpStream) -> (String, String) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let parts: Vec<&str> = first.split_whitespace().collect();
    let method = parts[0].to_string();
    let target = parts[1].to_string();

    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body).unwrap();
    }
    (method, target)
}

#[test]
fn cli_end_to_end_with_funsecret_credentials() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let repo_url = format!("http://{addr}/api/protocol/org1/generic/demo");

    // 6 requests:
    // 0: exist -> 404 (not found, exit 1)
    // 1: upload pre-check HEAD -> 404
    // 2: upload POST -> 200 JSON with url
    // 3: exist -> 200 (found, exit 0)
    // 4: sign-url HEAD -> 200 with sign url header
    // 5: download GET -> 200 with payload
    let server = thread::spawn(move || {
        for step in 0..6 {
            let (mut stream, _) = listener.accept().unwrap();
            let (method, _target) = read_request(&mut stream);
            let response = match step {
                0 | 1 => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string(),
                2 => {
                    assert_eq!(method, "POST");
                    let body =
                        r#"{"successful":true,"object":{"url":"https://dl.example/app.tgz","fileMd5":"abc"}}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                }
                3 => "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                4 => "HTTP/1.1 200 OK\r\nx-artlab-generic-sign-url: https://signed.example/app.tgz\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                _ => {
                    let body = "downloaded-bytes";
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                }
            };
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
    });

    // 在独立临时目录写入加密 funsecret 凭据
    let temp = tempfile::tempdir().unwrap();
    let secret_home = temp.path().join("home");
    let secret_dir = secret_home.join(".secret");
    let manage = SecretManage::new(Some(secret_dir.to_str().unwrap()), None, None).unwrap();
    manage
        .write(
            &repo_url, "funpub", "aliyun", "generic", "demo", "repo_url", true, None,
        )
        .unwrap();
    manage
        .write(
            "alice", "funpub", "aliyun", "generic", "demo", "username", true, None,
        )
        .unwrap();
    manage
        .write(
            "secret-pw",
            "funpub",
            "aliyun",
            "generic",
            "demo",
            "password",
            true,
            None,
        )
        .unwrap();

    let bin = env!("CARGO_BIN_EXE_funpub");
    let run_cli = |args: &[&str]| {
        Command::new(bin)
            .args(args)
            .env("FUN_SECRET_PATH", &secret_home)
            .output()
            .unwrap()
    };

    // 1. exist -> not found (exit 1)
    let out = run_cli(&["exist", "a/b/app.tgz", "-v", "1.0.0", "--repo-name", "demo"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("不存在: a/b/app.tgz@1.0.0"));

    // 2. upload -> success (exit 0)
    let upload_file = temp.path().join("app.tgz");
    std::fs::write(&upload_file, b"payload").unwrap();
    let out = run_cli(&[
        "upload",
        upload_file.to_str().unwrap(),
        "a/b/app.tgz",
        "--version",
        "1.0.0",
        "--repo-name",
        "demo",
    ]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("上传成功: https://dl.example/app.tgz"));

    // 3. exist -> found (exit 0)
    let out = run_cli(&["exist", "a/b/app.tgz", "-v", "1.0.0", "-r", "demo"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("存在: a/b/app.tgz@1.0.0"));

    // 4. sign-url -> prints signed URL
    let out = run_cli(&[
        "sign-url",
        "a/b/app.tgz",
        "-v",
        "1.0.0",
        "-r",
        "demo",
        "--expiration-seconds",
        "120",
    ]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "https://signed.example/app.tgz"
    );

    // 5. download -> writes file into trailing-slash dir
    let dl_dir = format!("{}/", temp.path().join("downloads").display());
    let out = run_cli(&[
        "download",
        "a/b/app.tgz",
        "-v",
        "1.0.0",
        "-r",
        "demo",
        "-o",
        &dl_dir,
    ]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        std::fs::read(temp.path().join("downloads").join("app.tgz")).unwrap(),
        b"downloaded-bytes"
    );

    server.join().unwrap();
}
