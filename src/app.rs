use vello::peniko;
use arc_swap::ArcSwap;

use std::sync::Arc;

use crate::buffer::TextBuffer;
use crate::renderer::run;

pub struct Args {
    pub font_size: f32, 
    pub bg_color: peniko::Color, 
    pub fg_color: peniko::Color,
    pub font_data: &'static [u8],
    pub filename: Option<String>,
}

const FONT_DATA: &[u8] = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");

pub struct App {
    args: Args,
}

impl App {
    pub fn new(filename: Option<String>) -> Self {
        let font_size = 28.0;
        let bg_color = peniko::Color::rgb8(0xFA, 0xFA, 0xFA);
        let fg_color = peniko::Color::rgb8(0x0, 0x0, 0x0);
        let args = Args {font_size, bg_color, fg_color, font_data: FONT_DATA, filename};
        return App {args};
    }

    pub fn run(&self) {
        if let Some(file_path) = &self.args.filename {
            if let Ok(buffer) = TextBuffer::from_filename(&file_path) {
                let buffer = Arc::new(ArcSwap::from_pointee(buffer));
                run(&self.args, buffer);
            } else {
                log::error!("file doesn't exist");
                std::process::exit(1);
            }
        } else {
            if let Ok(buffer) = TextBuffer::from_blank() {
                let buffer = Arc::new(ArcSwap::from_pointee(buffer));
                run(&self.args, buffer);
            } else {
                log::error!("failed to create buffer");
                std::process::exit(1);
            }
        }
    }
}

