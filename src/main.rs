use rusttype::Font; 
use glutin::dpi::LogicalSize;
use glutin::event::WindowEvent::CloseRequested;
use glutin::event::Event;

use pager::render::terminal_render;
use pager::render::GlyphAtlas;
use pager::render::Display;

fn main() {
    let font_data = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");
    let font = Font::try_from_bytes(font_data).expect("Error loading font");
    let atlas = GlyphAtlas::from_font(font, 40.0);

    terminal_render(atlas.width, atlas.height, &atlas.buffer);

    run(atlas);
}

fn run(glyph_atlas: GlyphAtlas) {
    let size = LogicalSize {width: 800, height: 600};
    let wb = glutin::window::WindowBuilder::new().with_inner_size(size).with_title("MY boi");
    let event_loop = glutin::event_loop::EventLoop::new();

    let display = Display::new(&event_loop, glyph_atlas, wb);

    event_loop.run(move |ev, _, control_flow| {
        display.draw();

        let next_frame_time = std::time::Instant::now() + std::time::Duration::from_nanos(16_666_667); // 1/60 of a second
        *control_flow = glutin::event_loop::ControlFlow::WaitUntil(next_frame_time);
        match ev {
            Event::WindowEvent { event, .. } => match event {
                CloseRequested => {
                    *control_flow = glutin::event_loop::ControlFlow::Exit;
                    return;
                },
                _ => (),
            },
            _ => (),
        }
    });

}
