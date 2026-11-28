//! funpub 命令行入口与子命令实现。

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand};
use funpub_core::{
    create_publisher, list_available_channels, list_missing_channels, ChannelConfig,
    DownloadTarget, FunPubError, Publisher, UploadOptions, DEFAULT_REPO_TYPE,
};

#[derive(Parser, Debug)]
#[command(name = "funpub", version, about = "funpub 多渠道制品发布命令行工具")]
pub struct Cli {
    /// 打印更详细的运行日志。
    #[arg(long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 上传制品文件。
    #[command(disable_version_flag = true)]
    Upload(UploadArgs),

    /// 下载制品文件。
    #[command(disable_version_flag = true)]
    Download(DownloadArgs),

    /// 检查指定版本的远端制品是否存在（存在退出码 0，不存在退出码 1）。
    #[command(disable_version_flag = true)]
    Exist(ExistArgs),

    /// 生成临时免密下载地址。
    #[command(name = "sign-url", disable_version_flag = true)]
    SignUrl(SignUrlArgs),

    /// 列出所有已注册的发布渠道及其可用状态。
    Channels,
}

#[derive(Args, Debug, Clone)]
pub struct RepoArgs {
    /// 发布渠道。
    #[arg(short = 'c', long, default_value = "aliyun")]
    pub channel: String,

    /// 仓库名称，用于从 funsecret 按 (repo-type, repo-name) 查找 repo_url/username/password。
    #[arg(short = 'r', long = "repo-name")]
    pub repo_name: Option<String>,

    /// 仓库类型，默认 generic。
    #[arg(long = "repo-type", default_value = DEFAULT_REPO_TYPE)]
    pub repo_type: String,

    /// 仓库地址，未传则按 repo-name/repo-type 从 funsecret 读取。
    #[arg(long = "repo-url")]
    pub repo_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct UploadArgs {
    /// 本地文件路径。
    pub file: PathBuf,

    /// 远端制品路径。
    pub path: String,

    /// 制品版本号。
    #[arg(short = 'v', long = "version", required = true)]
    pub artifact_version: String,

    /// 制品名称，默认取本地文件名。
    #[arg(long)]
    pub filename: Option<String>,

    /// 版本描述。
    #[arg(long)]
    pub description: Option<String>,

    /// 是否覆盖已存在的同版本制品。
    #[arg(long)]
    pub overwrite: bool,

    #[command(flatten)]
    pub repo: RepoArgs,
}

#[derive(Args, Debug)]
pub struct DownloadArgs {
    /// 远端制品路径。
    pub path: String,

    /// 制品版本号。
    #[arg(short = 'v', long = "version", required = true)]
    pub artifact_version: String,

    /// 保存目录或文件路径。
    #[arg(short = 'o', long, default_value = ".")]
    pub output: String,

    /// 是否覆盖已存在的本地文件。
    #[arg(long)]
    pub overwrite: bool,

    #[command(flatten)]
    pub repo: RepoArgs,
}

#[derive(Args, Debug)]
pub struct ExistArgs {
    /// 远端制品路径。
    pub path: String,

    /// 制品版本号。
    #[arg(short = 'v', long = "version", required = true)]
    pub artifact_version: String,

    #[command(flatten)]
    pub repo: RepoArgs,
}

#[derive(Args, Debug)]
pub struct SignUrlArgs {
    /// 远端制品路径。
    pub path: String,

    /// 制品版本号。
    #[arg(short = 'v', long = "version", required = true)]
    pub artifact_version: String,

    /// 有效期（秒），默认 3600，必须 >= 1。
    #[arg(
        long = "expiration-seconds",
        default_value_t = 3600,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub expiration_seconds: u64,

    #[command(flatten)]
    pub repo: RepoArgs,
}

fn init_logging(verbose: bool) {
    let default_level = if verbose { "info" } else { "warn" };
    let env = env_logger::Env::default().default_filter_or(default_level);
    let _ = env_logger::Builder::from_env(env)
        .format_timestamp_secs()
        .try_init();
}

fn build_publisher(repo: &RepoArgs) -> funpub_core::Result<Box<dyn Publisher>> {
    let mut cfg = ChannelConfig::default();
    if let Some(ref name) = repo.repo_name {
        if !name.is_empty() {
            cfg.repo_name = Some(name.clone());
            cfg.repo_type = repo.repo_type.clone();
        }
    }
    if let Some(ref url) = repo.repo_url {
        if !url.is_empty() {
            cfg.repo_url = Some(url.clone());
        }
    }
    create_publisher(&repo.channel, cfg)
}

fn remote_basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or("")
}

fn run_with_writers<I, T, WOut, WErr>(args: I, out: &mut WOut, err: &mut WErr) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
    WOut: Write,
    WErr: Write,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let code = e.exit_code();
            let _ = if code == 0 {
                write!(out, "{e}")
            } else {
                write!(err, "{e}")
            };
            return code;
        }
    };

    init_logging(cli.verbose);

    match execute(cli.command, out) {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(err, "错误: {e}");
            1
        }
    }
}

fn execute<WOut: Write>(command: Command, out: &mut WOut) -> funpub_core::Result<i32> {
    match command {
        Command::Upload(args) => {
            if !args.file.exists() {
                return Err(FunPubError::FileNotFound(args.file));
            }
            if args.file.is_dir() {
                return Err(FunPubError::IsADirectory(args.file));
            }
            let publisher = build_publisher(&args.repo)?;
            let options = UploadOptions {
                filename: args.filename,
                description: args.description,
                overwrite: args.overwrite,
                ..UploadOptions::default()
            };
            let result =
                publisher.upload_file(&args.file, &args.path, &args.artifact_version, &options)?;
            match result.url.as_deref() {
                Some(url) if !url.is_empty() => {
                    let _ = writeln!(out, "上传成功: {url}");
                }
                _ => {
                    let _ = writeln!(out, "上传成功: {}@{}", args.path, args.artifact_version);
                }
            }
            Ok(0)
        }
        Command::Download(args) => {
            let output_path = Path::new(&args.output);
            let is_dir_target = output_path.is_dir()
                || args.output.ends_with('/')
                || args.output.ends_with(std::path::MAIN_SEPARATOR);

            let publisher = build_publisher(&args.repo)?;
            let (target, display_saved) = if is_dir_target {
                let target = DownloadTarget {
                    save_dir: Some(output_path.to_path_buf()),
                    filename: None,
                    filepath: None,
                    overwrite: args.overwrite,
                };
                let saved = output_path.join(remote_basename(&args.path));
                (target, saved)
            } else {
                let target = DownloadTarget {
                    save_dir: None,
                    filename: None,
                    filepath: Some(output_path.to_path_buf()),
                    overwrite: args.overwrite,
                };
                (target, output_path.to_path_buf())
            };

            publisher.download_file(&args.path, &args.artifact_version, &target)?;
            let _ = writeln!(out, "下载完成: {}", display_saved.display());
            Ok(0)
        }
        Command::Exist(args) => {
            let publisher = build_publisher(&args.repo)?;
            let found = publisher.exist(&args.path, &args.artifact_version)?;
            if found {
                let _ = writeln!(out, "存在: {}@{}", args.path, args.artifact_version);
                Ok(0)
            } else {
                let _ = writeln!(out, "不存在: {}@{}", args.path, args.artifact_version);
                Ok(1)
            }
        }
        Command::SignUrl(args) => {
            let publisher = build_publisher(&args.repo)?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            let expiration_ms =
                (now + Duration::from_secs(args.expiration_seconds)).as_millis() as u64;
            let url = publisher.get_download_url(
                &args.path,
                &args.artifact_version,
                Some(expiration_ms),
            )?;
            let _ = writeln!(out, "{url}");
            Ok(0)
        }
        Command::Channels => {
            let available = list_available_channels();
            let missing = list_missing_channels();
            let mut lines: Vec<(String, String)> = Vec::new();
            for spec in available {
                lines.push((spec.name.to_string(), "available".to_string()));
            }
            for (name, hint) in missing {
                lines.push((name, format!("missing ({hint})")));
            }
            lines.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, status) in lines {
                let _ = writeln!(out, "{name}: {status}");
            }
            Ok(0)
        }
    }
}

/// CLI 主入口，返回进程退出码。
pub fn run<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    run_with_writers(args, &mut stdout, &mut stderr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invoke(args: &[&str]) -> (i32, String, String) {
        let mut full = vec!["funpub"];
        full.extend_from_slice(args);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_with_writers(full, &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    #[test]
    fn cli_version_and_channels() {
        let (code, stdout, _) = invoke(&["--version"]);
        assert_eq!(code, 0);
        assert!(stdout.contains(env!("CARGO_PKG_VERSION")));

        let (code, stdout, _) = invoke(&["channels"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("aliyun: available"));
    }

    #[test]
    fn cli_rejects_directory_upload_and_zero_expiration() {
        let dir = tempfile::tempdir().unwrap();
        let (code, _, stderr) =
            invoke(&["upload", dir.path().to_str().unwrap(), "a/b", "-v", "1.0.0"]);
        assert_ne!(code, 0);
        assert!(stderr.contains("错误:"));

        let (code, _, _) = invoke(&[
            "sign-url",
            "a/b",
            "-v",
            "1.0.0",
            "--expiration-seconds",
            "0",
        ]);
        assert_ne!(code, 0);
    }

    #[test]
    fn cli_parses_subcommand_version_flag_without_conflict() {
        let cli = Cli::try_parse_from([
            "funpub",
            "exist",
            "a/b",
            "--version",
            "1.2.3",
            "--repo-name",
            "demo",
        ])
        .unwrap();
        match cli.command {
            Command::Exist(args) => {
                assert_eq!(args.path, "a/b");
                assert_eq!(args.artifact_version, "1.2.3");
                assert_eq!(args.repo.repo_name.as_deref(), Some("demo"));
                assert_eq!(args.repo.repo_type, "generic");
                assert_eq!(args.repo.channel, "aliyun");
            }
            _ => panic!("expected exist subcommand"),
        }
    }
}
