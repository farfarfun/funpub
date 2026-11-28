fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(funpub::run(std::env::args_os()) as u8)
}
