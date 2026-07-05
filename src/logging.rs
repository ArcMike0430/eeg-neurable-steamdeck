use env_logger::Env;

pub fn init_logging() {
    let env = Env::default().filter_or("RUST_LOG", "info");
    let _ = env_logger::Builder::from_env(env)
        .format_timestamp_millis()
        .try_init();
}
