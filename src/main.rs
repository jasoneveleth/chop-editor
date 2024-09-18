use std::env;
use std::fs::File;
use std::io::Write;
use chrono;
use log::LevelFilter;
use env_logger::Builder;

use chop::app::App;

fn init_logging(log_file: Option<&str>) {
    if let None = log_file {
        env_logger::init();
        return;
    }

    let file = File::create(log_file.expect("compiler should know better")).expect("Could not create log file");
    let mut builder = Builder::new();
    
    builder
        .filter(None, LevelFilter::Info)
        .write_style(env_logger::WriteStyle::Always)
        .format(|buf, record| {
            writeln!(
                buf,
                "{} - {} - {}",
                chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .target(env_logger::Target::Pipe(Box::new(file)));

    builder.init();
}

fn main() {
    init_logging(Some("/tmp/app.log"));
    let args: Vec<String> = env::args().collect();
    let filename = if args.len() <= 1 {
        None
    } else {
        Some(args[1].clone())
    };

    let app = App::new(filename);
    app.run();
}
