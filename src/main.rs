use std::{ffi::c_void, ptr};

use gl::types::{GLint, GLuint, GLsizeiptr, GLsizei, GLvoid, GLenum, GLchar};
use glutin::{event_loop::{EventLoop, ControlFlow}, window::WindowBuilder, ContextBuilder, GlRequest, Api, event::{Event, WindowEvent}};

use rusttype::{Font, Scale, point};

const VERTEX_SHADER_SOURCE: &str = r#"
#version 330 core

layout (location = 0) in vec2 position;
layout (location = 1) in vec2 tex_coords;

out vec2 frag_tex_coords;

void main()
{
    gl_Position = vec4(position, 0.0, 1.0);
    frag_tex_coords = tex_coords;
}
"#;

const FRAGMENT_SHADER_SOURCE: &str = r#"
#version 330 core

in vec2 frag_tex_coords;

uniform sampler2D tex;

out vec4 frag_color;

void main()
{
    frag_color = texture(tex, frag_tex_coords);
}
"#;

fn compile_shader(source: &str, shader_type: GLenum) -> GLuint {
    unsafe {
        let shader = gl::CreateShader(shader_type);
        gl::ShaderSource(shader, 1, &(source.as_ptr() as *const _), &(source.len() as GLint));
        gl::CompileShader(shader);

        // Check for shader compilation errors (not shown here for brevity)

        shader
    }
}

fn link_program(vertex_shader: GLuint, fragment_shader: GLuint) -> GLuint {
    unsafe {
        let program = gl::CreateProgram();
        gl::AttachShader(program, vertex_shader);
        gl::AttachShader(program, fragment_shader);
        gl::LinkProgram(program);

        // Check for program linking errors (not shown here for brevity)

        gl::DeleteShader(vertex_shader);
        gl::DeleteShader(fragment_shader);

        program
    }
}

fn main() {
    let font_data = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");
    let font = Font::try_from_bytes(font_data).expect("Error loading font");
    let glyph = font.glyph('A');
    let glyph = glyph.scaled(Scale::uniform(16.0));

    let advance_width = glyph.h_metrics().advance_width;
    let left_side_bearing = glyph.h_metrics().left_side_bearing;

    // Adjust the glyph position so min is (0, 0)
    let glyph = glyph.positioned(point(0.0, 0.0));
    let bbox = glyph.pixel_bounding_box().unwrap();
    let offset_x = bbox.min.x as f32;
    let offset_y = bbox.min.y as f32;
    let glyph = glyph.into_unpositioned();
    let glyph = glyph.positioned(point(-offset_x, -offset_y));

    let bbox = glyph.pixel_bounding_box().unwrap();
    let width = bbox.width() as u32;
    let height = bbox.height() as u32;

    // create bitmap to store the glyph's pixel data
    let mut buffer = vec![0u8; (width * height) as usize];

    glyph.draw(|x, y, v| {
        let x = x + bbox.min.x as u32;
        let y = y + bbox.min.y as u32;
        println!("{} * {} + {}", y, width, x);
        let index = (y * width + x) as usize;
        buffer[index] = (v * 255.0) as u8; // Store the alpha value in the buffer
    });

    run(width, height, buffer);
}

fn run(width: u32, height: u32, bitmap: Vec<u8>) {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().with_title("My Boy");

    let gl_context = ContextBuilder::new()
        .with_gl(GlRequest::Specific(Api::OpenGl, (3, 3)))
        .build_windowed(window, &event_loop)
        .expect("Cannot create windowed context");

    let gl_context = unsafe {
        gl_context
            .make_current()
            .expect("Failed to make context current")
    };
    gl::load_with(|ptr| gl_context.get_proc_address(ptr) as *const _);


    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        let vertex_shader = compile_shader(VERTEX_SHADER_SOURCE, gl::VERTEX_SHADER);
        let fragment_shader = compile_shader(FRAGMENT_SHADER_SOURCE, gl::FRAGMENT_SHADER);
        let shader_program = link_program(vertex_shader, fragment_shader);

        match event {
            Event::LoopDestroyed => (),
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::Resized(physical_size) => gl_context.resize(physical_size),
                _ => (),
            },
            Event::RedrawRequested(_) => {
                // generate an id for texture
                let mut texture_id: GLuint = 0;
                unsafe {
                    gl::GenTextures(1, &mut texture_id);
                    gl::BindTexture(gl::TEXTURE_2D, texture_id);
                    // Set texture parameters (optional)
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as GLint);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as GLint);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as GLint);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as GLint);
                }

                // put bitmap on texture
                unsafe {
                    gl::TexImage2D(
                        gl::TEXTURE_2D,
                        0, // Mipmap level
                        gl::RGBA as GLint, // Internal format
                        width as GLint,
                        height as GLint,
                        0, // Border (always 0)
                        gl::RGBA, // Format of the pixel data
                        gl::UNSIGNED_BYTE, // Type of the pixel data
                        bitmap.as_ptr() as *const c_void, // Pointer to the pixel data
                    );
                    gl::GenerateMipmap(gl::TEXTURE_2D);
                }

                // Create the vertex buffer object (VBO) and vertex array object (VAO) for the textured quad
                let vertices: [f32; 16] = [
                    // Positions      // Texture coordinates
                    -0.5, -0.5,       0.0, 0.0,
                    0.5, -0.5,        1.0, 0.0,
                    0.5, 0.5,         1.0, 1.0,
                    -0.5, 0.5,        0.0, 1.0,
                ];

                let mut vbo: GLuint = 0;
                let mut vao: GLuint = 0;
                unsafe {
                    gl::GenBuffers(1, &mut vbo);
                    gl::GenVertexArrays(1, &mut vao);

                    gl::BindVertexArray(vao);
                    gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
                    gl::BufferData(
                        gl::ARRAY_BUFFER,
                        (vertices.len() * std::mem::size_of::<f32>()) as GLsizeiptr,
                        vertices.as_ptr() as *const GLvoid,
                        gl::STATIC_DRAW,
                    );

                    // Set the vertex attribute pointers
                    let stride = (4 * std::mem::size_of::<f32>()) as GLsizei;
                    gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, stride, ptr::null());
                    gl::EnableVertexAttribArray(0);
                    gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, stride, (2 * std::mem::size_of::<f32>()) as *const GLvoid);
                    gl::EnableVertexAttribArray(1);

                    gl::BindVertexArray(0);
                    gl::BindBuffer(gl::ARRAY_BUFFER, 0);
                }

                // Bind and activate the texture
                unsafe {
                    gl::UseProgram(shader_program);
                    gl::BindVertexArray(vao);

                    // Set the texture uniform
                    let tex_uniform_location = gl::GetUniformLocation(shader_program, "tex\0".as_ptr() as *const GLchar);
                    gl::Uniform1i(tex_uniform_location, 0); // Texture unit 0

                    gl::ActiveTexture(gl::TEXTURE0);
                    gl::BindTexture(gl::TEXTURE_2D, texture_id);

                    // Render the quad
                    gl::DrawArrays(gl::TRIANGLE_FAN, 0, 4);

                    gl::BindVertexArray(0);
                    gl::UseProgram(0);
                }

                // unsafe {
                //     gl::ClearColor(0.0, 0.0, 1.0, 1.0);
                //     gl::Clear(gl::COLOR_BUFFER_BIT);
                // }
                gl_context.swap_buffers().unwrap();
            }
            _ => (),
        }
    });
}
