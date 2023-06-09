use glium::Surface;
use glium::implement_vertex;
use glium::uniform;
use glutin::dpi::LogicalSize;
use rusttype::Font; 
use rusttype::Scale; 
use rusttype::point;

use glium::{Texture2d, Program, VertexBuffer, IndexBuffer};
use glium::index::PrimitiveType;
use std::borrow::Cow;

fn terminal_render(width: usize, height: usize, buffer: &[u8]) {
    for y in (0..height).rev() {
        for x in 0..width {
            let r = buffer[(y * width + x)*4];
            let _g = buffer[(y * width + x)*4 + 1];
            let _b = buffer[(y * width + x)*4 + 2];
            let _a = buffer[(y * width + x)*4 + 3];
            let char = [" ", ":", "|", "O", "W"][(r / 52) as usize];
            print!("{char}{char}");
        }
        print!("\n");
    }
}

fn main() {
    let font_data = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");
    let font = Font::try_from_bytes(font_data).expect("Error loading font");
    let glyph = font.glyph('A');
    let glyph = glyph.scaled(Scale::uniform(16.0));

    let _advance_width = glyph.h_metrics().advance_width;
    let _left_side_bearing = glyph.h_metrics().left_side_bearing;

    let glyph = glyph.positioned(point(0.0, 0.0));
    let bbox = glyph.pixel_bounding_box().unwrap();

    let width = bbox.width() as usize;
    let height = bbox.height() as usize;

    // create bitmap to store the glyph's pixel data
    let mut buffer = vec![0u8; width * height * 4]; // *4 for rgba

    glyph.draw(|x, y, v| {
        let x = x as usize;
        let y = height - y as usize - 1; // flip y over
        let index = (y * width + x) * 4;

        let v = (v * 255.0) as u8;
        let (r, g, b, a) = (v, v, v, 1);

        buffer[index] = r;
        buffer[index + 1] = g;
        buffer[index + 2] = b;
        buffer[index + 3] = a;
    });

    terminal_render(width, height, &buffer);

    run(width, height, buffer);
}

const VERTEX_SHADER_SOURCE: &str = r#"
    #version 330 core

    in vec2 position;
    in vec2 tex_coords;
    out vec2 v_tex_coords;

    void main() {
        gl_Position = vec4(position, 0.0, 1.0);
        v_tex_coords = tex_coords;
    }
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
    #version 330 core

    uniform sampler2D tex;
    in vec2 v_tex_coords;
    out vec4 color;

    void main() {
        color = texture(tex, v_tex_coords);
    }
"#;

#[derive(Copy, Clone)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

implement_vertex!(Vertex, position, tex_coords);

fn run(bitmap_width: usize, bitmap_height: usize, bitmap_data: Vec<u8>) {
    let size = LogicalSize {width: 800, height: 600};
    let event_loop = glutin::event_loop::EventLoop::new();
    let wb = glutin::window::WindowBuilder::new().with_inner_size(size).with_title("MY boi");
    let cb = glutin::ContextBuilder::new();
    let display = glium::Display::new(wb, cb, &event_loop).unwrap();

    let raw_image = glium::texture::RawImage2d::from_raw_rgba(bitmap_data, (bitmap_width as u32, bitmap_height as u32));

    // Create a texture from the bitmap data
    let texture = Texture2d::new(&display, raw_image).unwrap();

    // Compile the shaders and create the program
    let program = Program::from_source(&display, VERTEX_SHADER_SOURCE, FRAGMENT_SHADER_SOURCE, None).unwrap();

    // Create a vertex buffer for the quad
    let vertex_buffer = VertexBuffer::new(&display, &[
        Vertex { position: [-0.5, -0.5], tex_coords: [0.0, 0.0] },
        Vertex { position: [-0.5,  0.5], tex_coords: [0.0, 1.0] },
        Vertex { position: [ 0.5,  0.5], tex_coords: [1.0, 1.0] },
        Vertex { position: [ 0.5, -0.5], tex_coords: [1.0, 0.0] },
    ]).unwrap();

    // Create an index buffer for the quad
    let index_buffer = IndexBuffer::new(&display, PrimitiveType::TriangleStrip, &[1u32, 2u32, 0u32, 3u32]).unwrap();

    event_loop.run(move |ev, _, control_flow| {
        let mut target = display.draw();
        target.clear_color(0.0, 0.0, 0.0, 1.0);

        // Bind the vertex buffer, index buffer, texture, and program
        target.draw(
            &vertex_buffer,
            &index_buffer,
            &program,
            &uniform! { tex: &texture },
            &Default::default(),
        ).unwrap();

        // Finish the frame
        target.finish().unwrap();

        let next_frame_time = std::time::Instant::now() + std::time::Duration::from_nanos(16_666_667);
        *control_flow = glutin::event_loop::ControlFlow::WaitUntil(next_frame_time);
        match ev {
            glutin::event::Event::WindowEvent { event, .. } => match event {
                glutin::event::WindowEvent::CloseRequested => {
                    *control_flow = glutin::event_loop::ControlFlow::Exit;
                    return;
                },
                _ => (),
            },
            _ => (),
        }
    });

}
