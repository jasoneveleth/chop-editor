use std::collections::HashMap;
use std::iter::Scan;

use glium::glutin::surface::WindowSurface;
use glium::program::ProgramCreationInput;
use winit::dpi::LogicalSize;
use log::error;
use log::warn;
use rusttype::Glyph;
use rusttype::GlyphId;
use rusttype::PositionedGlyph;
use rusttype::Scale; 
use rusttype::point;
use rusttype::Font; 

use glium::Surface;
use glium::implement_vertex;
use glium::uniform;
use glium::{Program, VertexBuffer, IndexBuffer};
use glium::texture::Texture2d;
use glium::index::PrimitiveType;
use glium::Blend;
use unicode_segmentation::Graphemes;
use std::ops::Add;

use log::debug;

use term_size;

use crate::buffer::TextBuffer;

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

fn char2hex(c: char) -> String {
    let mut buf = [0u8; 4];
    let bytes = c.encode_utf8(&mut buf);
    bytes.bytes().fold(String::new(), |acc, byte| acc + &format!("\\x{:x}", byte))
}

// Function to check if a character is a variation selector
fn is_variation_selector(c: &char) -> bool {
    let variation_selector_range = '\u{FE00}'..='\u{FE0F}';
    variation_selector_range.contains(c)
}

impl GlyphAtlas {
    pub fn from_font(font: &Font, font_size: f32, font_color: (f32, f32, f32)) -> Self {
        let scale = Scale::uniform(font_size);

        // let all_chars = "ab❤️";
        let all_chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ`1234567890-=~!@#$%^&*()_+[]\\{}|;':\",./<>?";

        // we are skipping variation selectors for emojis since I'm not very smart
        let all_glyphs: Vec<rusttype::PositionedGlyph> = all_chars.chars().filter(|x| !is_variation_selector(x)).map(|c| {
            let glyph = font.glyph(c);

            // the glyph id for a glyph that isn't defined is 0
            let notdef_id = rusttype::GlyphId(0);
            let glyph = if glyph.id() == notdef_id {
                warn!("`{c}` with bytes: {} not found in font!", char2hex(c));

                // EMOJI: this is slow, but finds the emojis (but we can't seem to use them, so...)
                let font_data = include_bytes!("/System/Library/Fonts/Apple Color Emoji.ttc");
                let font = Font::try_from_bytes(font_data).expect("Error loading font");
                let glyph = font.glyph(c);
                if glyph.id() == notdef_id {
                    warn!("fuck it isn't an emoji");
                }
                glyph
            } else {
                glyph
            };

            let glyph = glyph.scaled(scale);
            glyph.positioned(point(0.0, 0.0))
        }).collect();

        let width_pixels = all_glyphs.iter().enumerate().map(|(i, g)| {
            let width = g.pixel_bounding_box().map(|x| x.width()).unwrap_or_else(|| {
                let c = all_chars.chars().nth(i).unwrap_or('?');
                error!("couldn't generate the bounding box for char (or ? if unwrap failed): `{}`, bytes: {}", c, char2hex(c));
                0});
            width
        }).sum::<i32>() as usize;
        let width_pixels = width_pixels + all_chars.len();

        let height_pixels = all_glyphs.iter().enumerate().map(|(i, g)| {
            let height = g.pixel_bounding_box().map(|x| x.height()).unwrap_or_else(|| {
                let c = all_chars.chars().nth(i).unwrap_or('?');
                error!("couldn't generate the bounding box for char (or ? if unwrap failed): `{}`, bytes: {}", c, char2hex(c));
                0});
            height
        }).max().unwrap() as usize;


        let mut map = HashMap::new();

        // create bitmap to store the glyph's pixel data
        let mut buffer = vec![0u8; width_pixels * height_pixels * 4]; // *4 for rgba

        let mut curr_x = 0;
        for (c, glyph) in all_chars.chars().zip(all_glyphs) {
            if let Some(bbox) = glyph.pixel_bounding_box() {
                let glyph_width = bbox.width() as usize;
                let glyph_height = bbox.height() as usize;

                glyph.draw(|x, y, v| {
                    let x = curr_x + x as usize;
                    let y = glyph_height - y as usize - 1; // flip y over
                    let index = (y * width_pixels + x) * 4;

                    let v = (v * 255.0) as u8;
                    let (r, g, b) = font_color;
                    buffer[index] = (r * 255.0) as u8;
                    buffer[index + 1] = (g * 255.0) as u8;
                    buffer[index + 2] = (b * 255.0) as u8;
                    buffer[index + 3] = v;
                });

                let top_left = rusttype::point(curr_x, height_pixels - glyph_height);
                let bottom_right = rusttype::point(curr_x + glyph_width, height_pixels - 1);
                let dims = rusttype::Rect { min: top_left, max: bottom_right};

                let tex_pos = dims2pos(width_pixels, height_pixels, dims);
                map.insert(c, GlyphData{width: glyph_width, height: glyph_height, tex_pos});

                curr_x += glyph_width+1;
            } else {
                warn!("`{c}` with bytes: {} not found in font!", char2hex(c));
            }
        }

        Self {buffer, width: width_pixels, height: height_pixels, map}
    }
}

const FONT_VERTEX_SHADER_SOURCE: &str = r#"
    #version 330 core

    in vec2 position;
    in vec2 tex_coords;
    out vec2 v_tex_coords;

    void main() {
        gl_Position = vec4(position, 0.0, 1.0);
        v_tex_coords = tex_coords;
    }
"#;

const FONT_FRAGMENT_SHADER_SOURCE: &str = r#"
    #version 330 core

    uniform sampler2D tex;
    in vec2 v_tex_coords;
    out vec4 color;

    void main() {
        color = texture(tex, v_tex_coords);
    }
"#;

const RECTANGLE_VERTEX_SHADER_SOURCE: &str = r#"
    #version 140

    in vec2 position;

    void main() {
        gl_Position = vec4(position, 0.0, 1.0);
    }
"#;

const RECTANGLE_FRAGMENT_SHADER_SOURCE: &str = r#"
    #version 140

    uniform vec4 color;
    out vec4 c;

    void main() {
        c = color;
    }
"#;

#[derive(Copy, Clone)]
pub struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

implement_vertex!(Vertex, position, tex_coords);

#[derive(Copy, Clone, Debug)]
pub struct CursorVertex {
    position: [f32; 2],
}

implement_vertex!(CursorVertex, position);

pub struct Display {
    glium_display: glium::Display<WindowSurface>,
    rectangle_program: Program,
    font_program: Program,
    atlas_texture: Texture2d,
    glyph_info: GlyphInfo,
    size: LogicalSize<u32>,
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
    pub fn new(glyph_atlas: GlyphAtlas, display: glium::Display<WindowSurface>, size: LogicalSize<u32>) -> Self {
        terminal_render(glyph_atlas.width, glyph_atlas.height, &glyph_atlas.buffer);
        let raw_image = glium::texture::RawImage2d::from_raw_rgba(glyph_atlas.buffer, (glyph_atlas.width as u32, glyph_atlas.height as u32));
        let texture = Texture2d::new(&display, raw_image).expect("unable to create 2d texture");
        let font_program = Program::new(
            &display, 
            ProgramCreationInput::SourceCode {
                vertex_shader: FONT_VERTEX_SHADER_SOURCE, 
                fragment_shader: FONT_FRAGMENT_SHADER_SOURCE, 
                geometry_shader: None,
                tessellation_control_shader: None,
                tessellation_evaluation_shader: None,
                transform_feedback_varyings: None,
                outputs_srgb: true, // <- This seems to fix the issue
                uses_point_size: false,
            }).expect("unable to create program");

        let cursor_program = Program::new(
            &display, 
            ProgramCreationInput::SourceCode {
                vertex_shader: RECTANGLE_VERTEX_SHADER_SOURCE, 
                fragment_shader: RECTANGLE_FRAGMENT_SHADER_SOURCE, 
                geometry_shader: None,
                tessellation_control_shader: None,
                tessellation_evaluation_shader: None,
                transform_feedback_varyings: None,
                outputs_srgb: true, // <- This seems to fix the issue
                uses_point_size: false,
            }).expect("unable to create program");

        Self {size, glyph_info: glyph_atlas.map, glium_display: display, rectangle_program: cursor_program, font_program, atlas_texture: texture}
    }

    // The letter `T`
    //
    // |------------| ^
    // |MMMMMMMMMMMM| |
    // |     M      | |  v_metrics.ascent
    // |     M      | |
    // |     M      | |
    // | - - M - - -| - -  baseline
    // |     M      | | descent
    // |_____M______| v

    // source: https://git.xobs.io/xobs/rust-font-test/src/branch/master/src/main.rs line 281
    // The origin of a line of text is at the baseline (roughly where
    // non-descending letters sit). We don't want to clip the text, so we shift
    // it down with an offset when laying it out. v_metrics.ascent is the
    // distance between the baseline and the highest edge of any glyph in
    // the font. That's enough to guarantee that there's no clipping.
    fn graphemes2glyphs<'a>(&'a self, font: &Font<'a>, scale: Scale, text: Graphemes<'a>, offset_y: f32, offset_x: f32, total_line_height: f32) -> Scan<Graphemes, (Option<GlyphId>, f32), Box<dyn FnMut(&mut (Option<GlyphId>, f32), &str) -> Option<((PositionedGlyph<'a>, Option<&'a GlyphData>), (f32, f32))> + 'a>> {
        // the offset_y isn't to the top of the line, it's to the baseline 
        let mut line_start = point(offset_x, offset_y);
        let font = font.clone();
        let ascent = font.v_metrics(scale).ascent;

        text.scan((None, 0.0), Box::new(move |(prev, x): &mut (Option<GlyphId>, f32), g: &str| {
            // EMOJI: this may have more than 1 byte in it
            let ch = g.chars().nth(0).unwrap();
            let glyph_data = self.glyph_info.get(&ch);
            let glyph: Glyph = font.glyphs_for(g.chars()).to_owned().nth(0).unwrap();
            let g = glyph.scaled(scale);
            let mut left_side_of_char = *x;
            let top_of_char = line_start.y;
            if ch == '\n' {
                line_start = line_start + rusttype::vector(0.0, total_line_height);
                *x = 0.;
            } else {
                if let Some(prev) = prev {
                    let y: f32 = font.pair_kerning(scale, *prev, g.id());
                    *x += y;
                    left_side_of_char += y/2.;
                }
            }
            let w = g.h_metrics().advance_width;
            let next = g.positioned(line_start + rusttype::vector(*x, 0.0));
            *prev = Some(next.id());
            if ch != '\n' {
                *x += w;
            }
            Some(((next, glyph_data), (line_start.x + left_side_of_char, top_of_char - ascent)))
        }))
    }

    fn glyph_data2glyph_vertex_buffers(&self, glyphs: Vec<(PositionedGlyph, Option<&GlyphData>)>) -> (Vec<Vertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        let mut index: u32 = 0;
        for (glyph, glyph_data) in glyphs {
            if let Some(glyph_data) = glyph_data {
                if let Some(bbox) = glyph.pixel_bounding_box() {
                    let x = (bbox.min.x as f32 / self.size.width as f32) * 2.0 - 1.;
                    let y = ((bbox.min.y as f32) / self.size.height as f32) * -2.0 + 1.;
                    let top_left = rusttype::point(x, y);

                    let new_width = (glyph_data.width as f32 / self.size.width as f32) * 2.0;
                    let new_height = (glyph_data.height as f32 / self.size.height as f32) * 2.0;

                    vertices.push(Vertex { position: [top_left.x, top_left.y - new_height], tex_coords: glyph_data.tex_pos[0] });
                    vertices.push(Vertex { position: [top_left.x, top_left.y], tex_coords: glyph_data.tex_pos[1] });
                    vertices.push(Vertex { position: [top_left.x + new_width, top_left.y], tex_coords: glyph_data.tex_pos[2] });
                    vertices.push(Vertex { position: [top_left.x + new_width, top_left.y - new_height], tex_coords: glyph_data.tex_pos[3] });

                    // a list of triangles of vertex indices
                    // triangle 1
                    indices.push(index + 1);
                    indices.push(index + 2);
                    indices.push(index);
                    // triangle 2
                    indices.push(index);
                    indices.push(index + 2);
                    indices.push(index + 3);
                    index += 4;
            }
            }
        }
        (vertices, indices)
    }

    fn mid_glyph_positions2cursor_vertex_buffers(&self, buffer: &TextBuffer, glyph_positions: Vec<(f32, f32)>, line_height: f32, start_grapheme_index: usize, end_grapheme_index: usize) -> (Vec<CursorVertex>, Vec<u32>) {
        let window_size = self.size;
        let line_height = 2.*line_height/window_size.height as f32;

        let mut vertex_list = Vec::new();
        let mut triangle_list = Vec::new();
        for cursor in buffer.cursors.iter() {
            if start_grapheme_index <= cursor.start && cursor.start < end_grapheme_index {
                let (left, top) = glyph_positions[cursor.start - start_grapheme_index];
                let [top, left] = [-2.*top/window_size.height as f32+1., 2.*left/window_size.width as f32-1.];
                let index = vertex_list.len();
                let cursor_width = 2.*(1.7)/window_size.width as f32; // TODO: hardcoded cursor width based on glyph
                vertex_list.push(CursorVertex { position: [left - cursor_width, top - line_height]});
                vertex_list.push(CursorVertex { position: [left + cursor_width, top - line_height]});
                vertex_list.push(CursorVertex { position: [left + cursor_width, top]});
                vertex_list.push(CursorVertex { position: [left - cursor_width, top]});

                // a list of triangles of vertex indices
                // triangle 1
                triangle_list.push(index as u32);
                triangle_list.push((index+1) as u32);
                triangle_list.push((index+2) as u32);
                // triangle 2
                triangle_list.push(index as u32);
                triangle_list.push((index+2) as u32);
                triangle_list.push((index+3) as u32);
            }
        }
        (vertex_list, triangle_list)
    }

    pub fn draw(&self, font_size: f32, font: &Font, offset_y: f32, titlebar_height: f32, offset_x: f32, buffer: &TextBuffer, bg_color: (f32, f32, f32, f32)) -> Result<(), Box<dyn std::error::Error>> {
        let scale = rusttype::Scale::uniform(font_size);
        // 1. pick lines that are relevant -> iterator of lines (each is an iterator of graphemes)
        // 2. lines iterator -> glyph data + mid-glyph positions
        // 3. glyph data --> vertex + index buffers
        // 4. mid-glyph -> vertex + index buffers for cursors
        // 5. draw the vertex and index buffers

        // 1. ============================================================
        // 0,0 is top left, positive y goes down
        let window_size = self.size;
        let v_metrics = font.v_metrics(scale);
        let line_height = v_metrics.ascent - v_metrics.descent;
        let total_line_height = line_height + v_metrics.line_gap;
        // (line_nr+1)*(total_line_height) - y_offset > titlebar_height means next line starts below top of screen
        let start_line = ((offset_y + titlebar_height)/(total_line_height) - 1.).ceil().max(0.) as usize;
        // (line_nr)*(total_line_height) - y_offset > winow.height means line starts below bottom of screen
        let last_line = ((window_size.height as f32 + offset_y)/total_line_height).ceil() as usize;

        // 2. =============================================================
        let (lines_of_graphemes_iter, (start_grapheme_index, end_grapheme_index)) = buffer.nowrap_lines(start_line, last_line);
        let start_line_y_offset = total_line_height*start_line as f32 - offset_y + v_metrics.ascent;
        let scan_iter = self.graphemes2glyphs(font, scale, lines_of_graphemes_iter, start_line_y_offset, offset_x, total_line_height);
        let (glyph_data, mid_glyph_positions): (Vec<_>, Vec<_>) = scan_iter.unzip();

        // 3. =============================================================
        // these have all the vertices we want to pass to the GPU
        let (vertex_list, triangle_list) = self.glyph_data2glyph_vertex_buffers(glyph_data);
        let glyph_vertex_buffer = VertexBuffer::new(&self.glium_display, &vertex_list[..])?;
        let glyph_index_buffer = IndexBuffer::new(&self.glium_display, PrimitiveType::TrianglesList, &triangle_list[..])?;
        let glyph_uniform = uniform! { tex: &self.atlas_texture };

        // 4. =============================================================
        let (vertex_list, triangle_list) = self.mid_glyph_positions2cursor_vertex_buffers(buffer, mid_glyph_positions, line_height, start_grapheme_index, end_grapheme_index);
        let cursor_vertex_buffer = VertexBuffer::new(&self.glium_display, &vertex_list[..])?;
        let cursor_index_buffer = IndexBuffer::new(&self.glium_display, PrimitiveType::TrianglesList, &triangle_list[..])?;
        let cursor_uniform = uniform! {color: [1.0f32, 0.2, 0.2, 1.0]};


        // titlebar
        let coords = [[0., 0.], [window_size.width as f32, 0.], [0., titlebar_height], [window_size.width as f32, titlebar_height]];
        let coords = coords.map(|[a, b]| CursorVertex{position: [a/(window_size.width as f32)*2.-1., -b/(window_size.height as f32)*2.+1.]});
        let titlebar_vertex_buffer = VertexBuffer::new(&self.glium_display, &coords)?;
        let titlebar_index_buffer = IndexBuffer::new(&self.glium_display, PrimitiveType::TrianglesList, &[0u32, 1u32, 2u32, 1u32, 2u32, 3u32])?;
        let titlebar_uniform = uniform! {color: [0.5f32, 0.0, 0.2, 0.1]};

        // 5. =============================================================
        let mut target = self.glium_display.draw();
        target.clear_color_srgb(bg_color.0, bg_color.1, bg_color.2, bg_color.3);

        let glyph_draw_parameters = glium::DrawParameters {
            blend: Blend::alpha_blending(),
            smooth: Some(glium::draw_parameters::Smooth::Nicest),
            .. Default::default()
        };

        // Bind the vertex buffer, index buffer, texture, and program
        target.draw(&glyph_vertex_buffer, &glyph_index_buffer, &self.font_program, &glyph_uniform, &glyph_draw_parameters)?;
        target.draw(&cursor_vertex_buffer, &cursor_index_buffer, &self.rectangle_program, &cursor_uniform, &Default::default())?;
        target.draw(&titlebar_vertex_buffer, &titlebar_index_buffer, &self.rectangle_program, &titlebar_uniform, &Default::default())?;


        // ======================================================================
        // Finish the frame
        target.finish()?;
        Ok(())
    }
}
