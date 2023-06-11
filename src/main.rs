use rusttype::Font; 
use glutin::dpi::{LogicalSize, PhysicalPosition};
use glutin::event::WindowEvent::{CloseRequested, MouseWheel};
use glutin::event::{Event, MouseScrollDelta};
use log::info;

// use pager::render::terminal_render;
use pager::render::GlyphAtlas;
use pager::render::Display;

fn main() {
    env_logger::init();

    let font_data = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");
    let font = Font::try_from_bytes(font_data).expect("Error loading font");
    let size = 150.0;
    let atlas = GlyphAtlas::from_font(&font, size);

    // terminal_render(atlas.width, atlas.height, &atlas.buffer);

    run(atlas, font, size);
}

fn run(glyph_atlas: GlyphAtlas, font: Font<'static>, font_size: f32) {
    let size = LogicalSize {width: 800, height: 600};
    let title = "My Boi";

    let wb = glutin::window::WindowBuilder::new().with_inner_size(size).with_title(title);
    let event_loop = glutin::event_loop::EventLoop::new();
    let cb = glutin::ContextBuilder::new();
    let window = cb.build_windowed(wb, &event_loop).unwrap();

    let display = Display::new(glyph_atlas, window);
    let mut scroll_y = 0.0;

    event_loop.run(move |ev, _, control_flow| {
        let next_frame_time = std::time::Instant::now() + std::time::Duration::from_nanos(16_666_667); // 1/60 of a second
        *control_flow = glutin::event_loop::ControlFlow::WaitUntil(next_frame_time);
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
                        },
                        MouseScrollDelta::PixelDelta(PhysicalPosition{x: _, y}) => {
                            scroll_y += y as f32;

                            // BAD BAD copying code (should just redraw)
                            let scale = rusttype::Scale::uniform(font_size);
                            let line_num = 0;
                            let (verts1, triangles1) = display.add_text(&font, scale, &format!("yeehaw"), line_num, 0, scroll_y);
                            let (verts2, triangles2) = display.add_text(&font, scale, &format!("next line"), 1, verts1.len(), scroll_y);

                            let mut verts = Vec::new();
                            verts.extend(verts1);
                            verts.extend(verts2);

                            let mut triangles = Vec::new();
                            triangles.extend(triangles1);
                            triangles.extend(triangles2);
                            display.draw(verts, triangles);
                        },
                    }
                },
                _ => (),
            },
            Event::RedrawRequested(_window_id) => {
                info!("redraw requested");
                let scale = rusttype::Scale::uniform(font_size);
                let line_num = 0;
                let (verts1, triangles1) = display.add_text(&font, scale, &format!("yeehaw"), line_num, 0, scroll_y);
                let (verts2, triangles2) = display.add_text(&font, scale, &format!("next line"), 1, verts1.len(), scroll_y);

                let mut verts = Vec::new();
                verts.extend(verts1);
                verts.extend(verts2);

                let mut triangles = Vec::new();
                triangles.extend(triangles1);
                triangles.extend(triangles2);
                display.draw(verts, triangles);
            }
            _ => (),
        }
    });
}
