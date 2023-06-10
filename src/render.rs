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
    let chunk_size = 176/2; // remember we're printing double
    let num_chunk = width/chunk_size;
    for z in 0..num_chunk {
        for y in (0..height).rev() {
            for x in z*chunk_size..(z+1)*chunk_size {
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
    for y in (0..height).rev() {
        for x in num_chunk*chunk_size..width {
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
    position_in_atlas: [[f32; 2]; 4],
}

// We can get a bitmap from a character and a font
pub struct GlyphAtlas {
    pub width: usize,
    pub height: usize,
    pub buffer: Vec<u8>,
    map: HashMap<char, GlyphData>,
}

//         position.min
//            v                   (1,1)
// +----------+---+-------------+
// |          |   |             |
// |          |   |             |
// |          +---+             |    } height_pixels
// |              ^             |
// |       position.max         |
// |                            |
// +----------------------------+
// (0,0)      ^
//        width_pixels
//
// convert to coords with bottom left as (0,0), top right as (1,1). 
fn dims2pos(width_pixels: usize, height_pixels: usize, position: rusttype::Rect<usize>) -> [[f32; 2]; 4] {
    let top_left_new_coords = point(
        position.min.x as f32 / width_pixels as f32,
        (height_pixels - position.min.y) as f32 / height_pixels as f32,
    );
    let bot_right_new_coords = point(
        (position.max.x + 1) as f32 / width_pixels as f32,
        (height_pixels-1 - position.max.y) as f32 / height_pixels as f32,
    );

    [
        [top_left_new_coords.x, bot_right_new_coords.y],
        [top_left_new_coords.x, top_left_new_coords.y],
        [bot_right_new_coords.x, top_left_new_coords.y],
        [bot_right_new_coords.x, bot_right_new_coords.y],
    ]
}

impl GlyphAtlas {
    pub fn from_font(font: Font, scale: f32) -> Self {
        let all_chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ`1234567890-=~!@#$%^&*()_+[]\\{}|;':\",./<>?";
        let all_glyphs: Vec<rusttype::PositionedGlyph> = all_chars.chars().map(|c| {
            let glyph = font.glyph(c);
            let glyph = glyph.scaled(Scale::uniform(scale));
            glyph.positioned(point(0.0, 0.0))
        }).collect();

        let width_pixels = all_glyphs.iter().map(|g| {
            let width = g.pixel_bounding_box().unwrap().width();
            width
        }).sum::<i32>() as usize;
        let width_pixels = width_pixels + all_chars.len();

        let height_pixels = all_glyphs.iter().map(|g| {
            let height = g.pixel_bounding_box().unwrap().height();
            height
        }).max().unwrap() as usize;


        let mut map = HashMap::new();

        // create bitmap to store the glyph's pixel data
        let mut buffer = vec![0u8; width_pixels * height_pixels * 4]; // *4 for rgba

        let mut curr_x = 0;
        for (c, glyph) in all_chars.chars().zip(all_glyphs) {
            let bbox = glyph.pixel_bounding_box().unwrap();
            let glyph_width = bbox.width() as usize;
            let glyph_height = bbox.height() as usize;

            glyph.draw(|x, y, v| {
                let x = curr_x + x as usize;

                let y = glyph_height - y as usize - 1; // flip y over

                let index = (y * width_pixels + x) * 4;

                let v = (v * 255.0) as u8;
                let (r, g, b, a) = (v, v, v, 1);

                buffer[index] = r;
                buffer[index + 1] = g;
                buffer[index + 2] = b;
                buffer[index + 3] = a;
            });

            let top_left = rusttype::point(curr_x, height_pixels - glyph_height);
            let bottom_right = rusttype::point(curr_x + glyph_width, height_pixels - 1);
            let dims = rusttype::Rect { min: top_left, max: bottom_right};

            map.insert(c, GlyphData{width: glyph_width, height: glyph_height, position_in_atlas: dims2pos(width_pixels, height_pixels, dims)});

            curr_x += glyph_width+1;
        }

        Self {buffer, width: width_pixels, height: height_pixels, map}
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
        let glyph_data = glyph_atlas.map.get(&'B').unwrap();

        let top_left = rusttype::point(-0.2, 0.7);
        let new_width = (glyph_data.width as f32 / window_size.width as f32) * 2.0;
        let new_height = (glyph_data.height as f32 / window_size.height as f32) * 2.0;

        let tex_coords = glyph_data.position_in_atlas;

        // Create a vertex buffer for the quad
        let vertex_buffer = VertexBuffer::new(&display, &[
            Vertex { position: [top_left.x, top_left.y - new_height], tex_coords: tex_coords[0] },
            Vertex { position: [top_left.x, top_left.y], tex_coords: tex_coords[1] },
            Vertex { position: [top_left.x + new_width, top_left.y], tex_coords: tex_coords[2] },
            Vertex { position: [top_left.x + new_width, top_left.y - new_height], tex_coords: tex_coords[3] },
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
