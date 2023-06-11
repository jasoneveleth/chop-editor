use std::collections::HashMap;

use log::warn;
use rusttype::Scale; 
use rusttype::point;
use rusttype::Font; 

use glium::Surface;
use glium::implement_vertex;
use glium::uniform;
use glium::{Texture2d, Program, VertexBuffer, IndexBuffer};
use glium::index::PrimitiveType;
use glium::Blend;
use std::ops::Add;

use log::debug;

use im::Vector;
use term_size;

fn calc_chunk_size(default_val: usize) -> usize {
    let log_prefix_size = 43;

    let cols = match term_size::dimensions() {
        Some((w, _)) => w,
        None => default_val
    };
    // remember we're printing double
    (cols - log_prefix_size)/2
}

pub fn terminal_render(width: usize, height: usize, buffer: &[u8]) {
    let palette = [" ", ":", "|", "O", "W"];
    // overshoot non divisors so we don't get index error later
    let byte_val_divisor = 256/palette.len() + (256 % palette.len() != 0) as usize;

    debug!("alpha channel:");
    let chunk_size = calc_chunk_size(176);
    let num_chunk = width/chunk_size;
    for z in 0..num_chunk {
        for y in (0..height).rev() {
            let mut str = "".to_string();
            for x in z*chunk_size..(z+1)*chunk_size {
                let _r = buffer[(y * width + x)*4];
                let _g = buffer[(y * width + x)*4 + 1];
                let _b = buffer[(y * width + x)*4 + 2];
                let a = buffer[(y * width + x)*4 + 3];
                let char = palette[a as usize / byte_val_divisor];
                str += char;
                str += char;
            }
            debug!("{str}");
        }
    }
    for y in (0..height).rev() {
        let mut str = "".to_string();
        for x in num_chunk*chunk_size..width {
            let _r = buffer[(y * width + x)*4];
            let _g = buffer[(y * width + x)*4 + 1];
            let _b = buffer[(y * width + x)*4 + 2];
            let a = buffer[(y * width + x)*4 + 3];
            let char = palette[a as usize / byte_val_divisor];
            str += char;
            str += char;
        }
        debug!("{str}");
    }
}

pub struct GlyphData {
    width: usize,
    height: usize,
    tex_pos: [[f32; 2]; 4],
}

pub type GlyphInfo = HashMap<char, GlyphData>;

// We can get a bitmap from a character and a font
pub struct GlyphAtlas {
    pub width: usize,
    pub height: usize,
    pub buffer: Vec<u8>,
    map: GlyphInfo,
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
    pub fn from_font(font: &Font, font_size: f32) -> Self {
        let scale = Scale::uniform(font_size);

        let all_chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ`1234567890-=~!@#$%^&*()_+[]\\{}|;':\",./<>?";
        let all_glyphs: Vec<rusttype::PositionedGlyph> = all_chars.chars().map(|c| {
            let glyph = font.glyph(c);
            let glyph = glyph.scaled(scale);
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

                buffer[index] = 255;
                buffer[index + 1] = 255;
                buffer[index + 2] = 255;
                buffer[index + 3] = v;
            });

            let top_left = rusttype::point(curr_x, height_pixels - glyph_height);
            let bottom_right = rusttype::point(curr_x + glyph_width, height_pixels - 1);
            let dims = rusttype::Rect { min: top_left, max: bottom_right};

            let tex_pos = dims2pos(width_pixels, height_pixels, dims);
            map.insert(c, GlyphData{width: glyph_width, height: glyph_height, tex_pos});

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
pub struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

implement_vertex!(Vertex, position, tex_coords);

pub struct Display {
    glium_display: glium::Display,
    program: Program,
    texture: Texture2d,
    glyph_info: GlyphInfo,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Point<T> {
    x: T,
    y: T,
}

impl<T: Add<Output = T>> Add for Point<T> {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {x: self.x + rhs.x, y: self.y + rhs.y}
    }
}

impl Display {
    pub fn new(glyph_atlas: GlyphAtlas, window: glutin::WindowedContext<glutin::NotCurrent>) -> Self {
        let display = glium::Display::from_gl_window(window).unwrap();

        let raw_image = glium::texture::RawImage2d::from_raw_rgba(glyph_atlas.buffer, (glyph_atlas.width as u32, glyph_atlas.height as u32));
        let texture = Texture2d::new(&display, raw_image).unwrap();
        let program = Program::from_source(&display, VERTEX_SHADER_SOURCE, FRAGMENT_SHADER_SOURCE, None).unwrap();

        Self {glyph_info: glyph_atlas.map, glium_display: display, program, texture}
    }

    pub fn add_text(&self, font: &Font, scale: Scale, text: &str) -> (Vector<Vertex>, Vector<u32>) {
        let window_size = self.glium_display.gl_window().window().inner_size();

        let v_metrics = font.v_metrics(scale);
        let padding = 0.;
        let glyphs: Vec<_> = font.layout(text, scale, point(padding, padding + v_metrics.ascent)).collect();

        // // work out the layout size of fully rendered text
        // let glyphs_height = (v_metrics.ascent - v_metrics.descent).ceil() as u32;
        // let glyphs_width = {
        //     let min_x = glyphs
        //         .first()
        //         .map(|g| g.pixel_bounding_box().unwrap().min.x)
        //         .unwrap();
        //     let max_x = glyphs
        //         .last()
        //         .map(|g| g.pixel_bounding_box().unwrap().max.x)
        //         .unwrap();
        //     (max_x - min_x) as u32
        // };

        let mut vertices = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for (glyph, c) in glyphs.iter().zip(text.chars()) {
            let bbox = glyph.pixel_bounding_box().unwrap();

            let x = (bbox.min.x as f32 / window_size.width as f32) * 2.0;
            let y = ((window_size.height as f32 - bbox.min.y as f32) / window_size.height as f32) * 2.0;
            let top_left = rusttype::point(x-1., y-1.);

            if top_left.x <= -1. {
                warn!("top left of glyph is rendered off (left) screen")
            }
            if top_left.y <= -1. {
                warn!("top left of glyph is rendered off (bottom) screen")
            }
            if top_left.x >= 1. {
                warn!("top left of glyph is rendered off (right) screen")
            }
            if top_left.y >= 1. {
                warn!("top left of glyph is rendered off (top) screen")
            }

            let glyph_data = self.glyph_info.get(&c).unwrap();

            let new_width = (glyph_data.width as f32 / window_size.width as f32) * 2.0;
            let new_height = (glyph_data.height as f32 / window_size.height as f32) * 2.0;

            let index = vertices.len();

            vertices.push(Vertex { position: [top_left.x, top_left.y - new_height], tex_coords: glyph_data.tex_pos[0] });
            vertices.push(Vertex { position: [top_left.x, top_left.y], tex_coords: glyph_data.tex_pos[1] });
            vertices.push(Vertex { position: [top_left.x + new_width, top_left.y], tex_coords: glyph_data.tex_pos[2] });
            vertices.push(Vertex { position: [top_left.x + new_width, top_left.y - new_height], tex_coords: glyph_data.tex_pos[3] });

            // a list of triangles of vertex indices
            // triangle 1
            indices.push((index + 1) as u32);
            indices.push((index + 2) as u32);
            indices.push(index as u32);
            // triangle 2
            indices.push(index as u32);
            indices.push((index + 2) as u32);
            indices.push((index + 3) as u32);
        }
        (Vector::from(vertices), Vector::from(indices))
    }

    pub fn draw(&self, vertex_list: Vector<Vertex>, triangle_list: Vector<u32>) {
        // all the vertices we want to pass to the GPU
        let arr: Vec<Vertex> = vertex_list.iter().copied().collect();
        let vertex_buffer = VertexBuffer::new(&self.glium_display, &arr[..]).unwrap();

        // a list of triangles of vertex indices
        let arr: Vec<u32> = triangle_list.iter().copied().collect();
        let index_buffer = IndexBuffer::new(&self.glium_display, PrimitiveType::TrianglesList, &arr[..]).unwrap();

        let mut target = self.glium_display.draw();
        target.clear_color(0.0, 0.0, 0.0, 1.0);

        let draw_parameters  = glium::DrawParameters {
            blend: Blend::alpha_blending(),
            .. Default::default()
        };

        // Bind the vertex buffer, index buffer, texture, and program
        target.draw(
            &vertex_buffer,
            &index_buffer,
            &self.program,
            &uniform! { tex: &self.texture },
            &draw_parameters,
        ).unwrap();

        // Finish the frame
        target.finish().unwrap();
    }
}
