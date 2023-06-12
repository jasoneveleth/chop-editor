use std::fs::read_to_string;

use rusttype::Font; 
use glutin::dpi::{LogicalSize, PhysicalPosition};
use glutin::event::WindowEvent::{CloseRequested, MouseWheel, KeyboardInput, ModifiersChanged};
use glutin::event::{Event, MouseScrollDelta, VirtualKeyCode, ModifiersState};
use log::{info, warn, error};

// use pager::render::terminal_render;
use pager::render::GlyphAtlas;
use pager::render::Display;

fn main() {
    env_logger::init();

    let font_data = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");
    let font = Font::try_from_bytes(font_data).expect("Error loading font");
    let font_size = 150.0;
    let font_color = (0, 0, 0);
    let atlas = GlyphAtlas::from_font(&font, font_size, font_color);

    let file_path = "/tmp/test.txt";
    if let Ok(text) = read_to_string(file_path) {
        let text = text.lines().map(String::from).collect();
        // terminal_render(atlas.width, atlas.height, &atlas.buffer);
        run(atlas, font, font_size, text);
    } else {
        error!("file doesn't exist");
        std::process::exit(1);
    }
}

fn run(glyph_atlas: GlyphAtlas, font: Font<'static>, font_size: f32, text: Vec<String>) {
    let size = LogicalSize {width: 800, height: 600};
    let title = "My Boi";

    let wb = glutin::window::WindowBuilder::new().with_inner_size(size).with_title(title);
    let event_loop = glutin::event_loop::EventLoop::new();
    let cb = glutin::ContextBuilder::new();
    let window = cb.build_windowed(wb, &event_loop).expect("unable to create window");

    let display = Display::new(glyph_atlas, window);
    let mut scroll_y = 0.0;

    let mut modifier_state = ModifiersState::default();
    let background_color = (1.0, 1.0, 1.0, 1.0);
    // let background_color = (0.15686275, 0.17254902, 0.20392157, 1.0);

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
                            warn!("we don't expect a linedelta from mouse scroll on macos");
                        },
                        MouseScrollDelta::PixelDelta(PhysicalPosition{x: _, y}) => {
                            scroll_y += y as f32;
                            match display.draw(font_size, &font, scroll_y, &text, background_color) {
                                Err(err) => error!("problem drawing: {:?}", err),
                                _ => ()
                            }
                        },
                    }
                },
                KeyboardInput{ input, .. } => {
                    if let Some(VirtualKeyCode::W) = input.virtual_keycode {
                        if modifier_state.contains(ModifiersState::LOGO) {
                            // Cmd+W combination pressed
                            info!("close requested");
                            *control_flow = glutin::event_loop::ControlFlow::Exit;
                            return;
                        }
                    }
                },
                ModifiersChanged(state) => {
                    modifier_state = state;
                },
                _ => (),
            },
            Event::RedrawRequested(_window_id) => {
                info!("redraw requested");
                match display.draw(font_size, &font, scroll_y, &text, background_color) {
                    Err(err) => error!("problem drawing: {:?}", err),
                    _ => ()
                }
            }
            _ => (),
        }
    });
}
