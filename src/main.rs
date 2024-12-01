use std::env;
use std::fs::File;
use std::io::Write;
use chrono;
use log::LevelFilter;
use env_logger::Builder;
use winit::event_loop::EventLoop;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::NSApplication;
use objc2_foundation::MainThreadMarker;

use chop::app::App;
use chop::app::AppDelegate;

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
    // let file = Some("/tmp/app.log");
    let file = None;
    init_logging(file);
    let args: Vec<String> = env::args().collect();
    let filename = if args.len() <= 1 {
        None
    } else {
        Some(args[1].clone())
    };

    let event_loop = EventLoop::new().unwrap();

    // =================================== weird objc stuff
    let mtm = MainThreadMarker::new().unwrap();
    let delegate = AppDelegate::new(mtm);
    // Important: Call `sharedApplication` after `EventLoop::new`, doing it before is not yet supported.
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // ===================================

    log::info!("calling App::new()");
    let mut app = App::new(filename, event_loop.create_proxy());
    event_loop.run_app(&mut app).unwrap();
}
