//! 文件流式校验和计算（MD5 / SHA-1 / SHA-256）。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use md5::{Digest as Md5Digest, Md5};
use sha1::Sha1;
use sha2::Sha256;

use crate::error::Result;

const HASH_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// 一次流式扫描得到的文件大小与多算法摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChecksums {
    pub size: u64,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// 计算文件的十六进制小写 MD5 摘要。
pub fn file_md5(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; HASH_CHUNK_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// 计算文件的十六进制小写 SHA-1 摘要。
pub fn file_sha1(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; HASH_CHUNK_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// 计算文件的十六进制小写 SHA-256 摘要。
pub fn file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// 单次顺序读取计算文件的大小与 MD5 / SHA-1 / SHA-256 摘要。
pub fn compute_checksums(path: &Path) -> Result<FileChecksums> {
    let mut file = File::open(path)?;
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK_SIZE];
    let mut size: u64 = 0;

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let slice = &buf[..n];
        md5.update(slice);
        sha1.update(slice);
        sha256.update(slice);
        size += n as u64;
    }

    Ok(FileChecksums {
        size,
        md5: hex_lower(&md5.finalize()),
        sha1: hex_lower(&sha1.finalize()),
        sha256: hex_lower(&sha256.finalize()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_known_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.txt");
        std::fs::write(&path, b"hello world").unwrap();

        assert_eq!(file_md5(&path).unwrap(), "5eb63bbbe01eeed093cb22bb8f5acdc3");
        assert_eq!(
            file_sha1(&path).unwrap(),
            "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed"
        );
        assert_eq!(
            file_sha256(&path).unwrap(),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );

        let all = compute_checksums(&path).unwrap();
        assert_eq!(all.size, 11);
        assert_eq!(all.md5, "5eb63bbbe01eeed093cb22bb8f5acdc3");
        assert_eq!(all.sha1, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed");
        assert_eq!(
            all.sha256,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
