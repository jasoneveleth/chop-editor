use pager::render::terminal_render;
use pager::render::Bitmap;
use pager::render::Display;
use rusttype::Font; 

use glutin::event::WindowEvent::CloseRequested;
use glutin::event::Event;

fn main() {
    let font_data = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");
    let font = Font::try_from_bytes(font_data).expect("Error loading font");
    let bm = Bitmap::from_glyph(font, 'A');

    terminal_render(bm.width, bm.height, &bm.buffer);

    run(bm);
}

fn run(bitmap: Bitmap) {
    let event_loop = glutin::event_loop::EventLoop::new();

    let display = Display::new(&event_loop, bitmap);

    event_loop.run(move |ev, _, control_flow| {
        display.draw();

        let next_frame_time = std::time::Instant::now() + std::time::Duration::from_nanos(16_666_667);
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
