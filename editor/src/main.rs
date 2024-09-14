use std::sync::Arc;

use std::env;

use vello::peniko;
use arc_swap::ArcSwap;

use chop_editor::renderer::run;
use chop_editor::renderer::Args;
use chop_editor::buffer::TextBuffer;

const FONT_DATA: &[u8] = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() <= 1 {
        log::error!("not enough args provided");
        std::process::exit(1);
    }
    let file_path = &env::args().collect::<Vec<_>>()[1];

    let font_size = 28.0;
    let bg_color = peniko::Color::rgb8(0xFA, 0xFA, 0xFA);
    let fg_color = peniko::Color::rgb8(0x0, 0x0, 0x0);
    let args = Args {font_size, bg_color, fg_color, font_data: FONT_DATA};

    if let Ok(buffer) = TextBuffer::from_filename(file_path) {
        let buffer = Arc::new(ArcSwap::from_pointee(buffer));
        // terminal_render(atlas.width, atlas.height, &atlas.buffer);
        run(args, buffer);
    } else {
        log::error!("file doesn't exist");
        std::process::exit(1);
    }
}
