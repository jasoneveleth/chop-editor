use std::env;

use rusttype::Font; 
use glutin::dpi::{LogicalSize, PhysicalPosition};
use glutin::event::WindowEvent::{CloseRequested, MouseWheel, ModifiersChanged, ReceivedCharacter};
use glutin::event::{Event, MouseScrollDelta, ModifiersState};
use log::{info, warn, error, debug};

// use pager::render::terminal_render;
use pager::render::GlyphAtlas;
use pager::render::Display;
use pager::buffer::TextBuffer;

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len() <= 1 {
        error!("not enough args provided");
        std::process::exit(1);
    }
    let file_path = &args[1];

    let font_data = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");
    let font = Font::try_from_bytes(font_data).expect("Error loading font");
    let font_size = 50.0;
    // let font_color = (0x38, 0x3A, 0x42);
    let font_color = (0xab, 0xb2, 0xbf);
    let font_color = (font_color.0 as f32 / 255.0, font_color.1 as f32 / 255.0, font_color.2 as f32 / 255.0);
    let font_color = font_color;
    let atlas = GlyphAtlas::from_font(&font, font_size, font_color);

    if let Ok(buffer) = TextBuffer::from_filename(file_path) {
        // terminal_render(atlas.width, atlas.height, &atlas.buffer);
        run(atlas, font, font_size, buffer);
    } else {
        error!("file doesn't exist");
        std::process::exit(1);
    }
}

fn run(glyph_atlas: GlyphAtlas, font: Font<'static>, font_size: f32, mut buffer: TextBuffer) {
    let size = LogicalSize {width: 800, height: 600};
    let title = "My Boi";

    let wb = glutin::window::WindowBuilder::new().with_inner_size(size).with_title(title);
    let event_loop = glutin::event_loop::EventLoop::new();
    let cb = glutin::ContextBuilder::new();
    let window = cb.with_srgb(true).build_windowed(wb, &event_loop).expect("unable to create window");

    let display = Display::new(glyph_atlas, window);
    let mut scroll_y = 0.0;

    let mut modifier_state = ModifiersState::default();

    // let color = (0xFA, 0xFA, 0xFA);
    let color = (0x28, 0x2c, 0x34);
    // let color = ((color.0 as f32 / 255.0).powf(2.2), (color.1 as f32 / 255.0).powf(2.2), (color.2 as f32 / 255.0).powf(2.2));
    let color = ((color.0 as f32 / 255.0), (color.1 as f32 / 255.0), (color.2 as f32 / 255.0));
    let (r, g, b) = color;
    let background_color = (r, g, b, 1.0);

    event_loop.run(move |ev, _, control_flow| {
        // this ensures we come through the event loop again in at most 16ms (wait for 16ms or an
        // event, whichever is sooner)
        // let next_frame_time = std::time::Instant::now() + std::time::Duration::from_nanos(16_666_667);
        // *control_flow = glutin::event_loop::ControlFlow::WaitUntil(next_frame_time);

        match ev {
            Event::WindowEvent { event, .. } => match event {
                CloseRequested => {
                    info!("close requested");
                    *control_flow = glutin::event_loop::ControlFlow::Exit;
                    return;
                },
                MouseWheel{delta, ..} => {
                    match delta {
                        MouseScrollDelta::LineDelta(_, y) => {
                            // Adjust the scroll position based on the scroll delta
                            scroll_y += y * 20.0; // Adjust the scroll speed as needed
                            warn!("we don't expect a linedelta from mouse scroll on macOS, ignoring");
                        },
                        MouseScrollDelta::PixelDelta(PhysicalPosition{x: _, y}) => {
                            scroll_y += y as f32;
                            scroll_y = scroll_y.min(0f32);
                            match display.draw(font_size, &font, scroll_y, buffer.lines(), background_color) {
                                Err(err) => error!("problem drawing: {:?}", err),
                                _ => ()
                            }
                        },
                    }
                },
                ReceivedCharacter(ch) => {
                    let mut need_redraw = false;
                    match ch {
                        'w' => {
                            if modifier_state.contains(ModifiersState::LOGO) {
                                // Cmd+W combination pressed
                                info!("close requested");
                                *control_flow = glutin::event_loop::ControlFlow::Exit;
                                return;
                            } else {
                                buffer = buffer.insert("w");
                                need_redraw = true;
                            }
                        },
                        '\r' => {
                            buffer = buffer.insert("\n");
                            need_redraw = true;
                        }
                        _ => {
                            if ch.is_ascii() {
                                let text = &format!("{ch}");
                                debug!("{}", text);
                                buffer = buffer.insert(text);
                                need_redraw = true;
                            }
                        }
                    }
                    if need_redraw {
                        match display.draw(font_size, &font, scroll_y, buffer.lines(), background_color) {
                            Err(err) => error!("problem drawing: {:?}", err),
                            _ => ()
                        }
                    }
                }
                ModifiersChanged(state) => {
                    modifier_state = state;
                },
                _ => (),
            },
            Event::RedrawRequested(_window_id) => {
                info!("redraw requested");
                match display.draw(font_size, &font, scroll_y, buffer.lines(), background_color) {
                    Err(err) => error!("problem drawing: {:?}", err),
                    _ => ()
                }
            }
            _ => (),
        }
    });
}
