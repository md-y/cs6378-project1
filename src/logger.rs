use std::io::Write;

pub fn setup_logger() {
    env_logger::Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "[{}] - [{}] - {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.target(),
                record.args()
            )
        })
        .filter_level(log::LevelFilter::Info)
        .init();
}
