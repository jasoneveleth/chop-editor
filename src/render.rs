use std::collections::HashMap;

use rusttype::Scale; 
use rusttype::point;
use rusttype::Font; 

use glium::Surface;
use glium::implement_vertex;
use glium::uniform;

use glium::{Texture2d, Program, VertexBuffer, IndexBuffer};
use glium::index::PrimitiveType;

pub fn terminal_render(width: usize, height: usize, buffer: &[u8]) {
    println!("red channel:");
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

struct GlyphData {
    width: usize,
    height: usize,
    position: rusttype::Rect<i32>,
}

// We can get a bitmap from a character and a font
pub struct GlyphAtlas {
    pub width: usize,
    pub height: usize,
    pub buffer: Vec<u8>,
    map: HashMap<char, GlyphData>,
}

impl GlyphAtlas {
    pub fn from_glyph(font: Font, char: char, scale: f32) -> Self {
        let glyph = font.glyph(char);
        let glyph = glyph.scaled(Scale::uniform(scale));

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

        let mut map = HashMap::new();
        map.insert(char, GlyphData{width, height, position: bbox});

        Self {buffer, width, height, map}
    }
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

pub struct Display {
    glium_display: glium::Display,
    vertex_buffer: VertexBuffer<Vertex>,
    index_buffer: IndexBuffer<u32>,
    program: Program,
    texture: Texture2d,
}

impl Display {
    pub fn new(event_loop: &glutin::event_loop::EventLoop<()>, glyph_atlas: GlyphAtlas, wb: glutin::window::WindowBuilder) -> Self {
        let cb = glutin::ContextBuilder::new();
        let window = cb.build_windowed(wb, event_loop).unwrap();
        let display = glium::Display::from_gl_window(window).unwrap();

        let raw_image = glium::texture::RawImage2d::from_raw_rgba(glyph_atlas.buffer, (glyph_atlas.width as u32, glyph_atlas.height as u32));

        // Create a texture from the bitmap data
        let texture = Texture2d::new(&display, raw_image).unwrap();

        // Compile the shaders and create the program
        let program = Program::from_source(&display, VERTEX_SHADER_SOURCE, FRAGMENT_SHADER_SOURCE, None).unwrap();

        let window_size = display.gl_window().window().inner_size();
        let glyph_data = glyph_atlas.map.get(&'A').unwrap();

        let top_left = rusttype::point(-0.2, 0.7);
        let new_width = (glyph_data.width as f32 / window_size.width as f32) * 2.0;
        let new_height = (glyph_data.height as f32 / window_size.height as f32) * 2.0;

        // Create a vertex buffer for the quad
        let vertex_buffer = VertexBuffer::new(&display, &[
            Vertex { position: [top_left.x, top_left.y - new_height], tex_coords: [0.0, 0.0] },
            Vertex { position: [top_left.x, top_left.y], tex_coords: [0.0, 1.0] },
            Vertex { position: [top_left.x + new_width, top_left.y], tex_coords: [1.0, 1.0] },
            Vertex { position: [top_left.x + new_width, top_left.y - new_height], tex_coords: [1.0, 0.0] },
        ]).unwrap();

        // Create an index buffer for the quad
        let index_buffer = IndexBuffer::new(&display, PrimitiveType::TriangleStrip, &[1u32, 2u32, 0u32, 3u32]).unwrap();
        Self {glium_display: display, vertex_buffer, index_buffer, program, texture}
    }

    pub fn draw(&self) {
        let mut target = self.glium_display.draw();
        target.clear_color(0.0, 0.0, 0.0, 1.0);

        // Bind the vertex buffer, index buffer, texture, and program
        target.draw(
            &self.vertex_buffer,
            &self.index_buffer,
            &self.program,
            &uniform! { tex: &self.texture },
            &Default::default(),
        ).unwrap();

        // Finish the frame
        target.finish().unwrap();
    }
}
