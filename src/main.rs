use std::ffi::c_void;

use glutin::{event_loop::{EventLoop, ControlFlow}, window::WindowBuilder, ContextBuilder, GlRequest, Api, event::{Event, WindowEvent}};

use rusttype::{Font, Scale, point};

fn main() {
    let font_data = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");
    let font = Font::try_from_bytes(font_data).expect("Error loading font");
    let glyph = font.glyph('A');

    let glyph = glyph.scaled(Scale::uniform(10.0));

    let advance_width = glyph.h_metrics().advance_width;
    let left_side_bearing = glyph.h_metrics().left_side_bearing;

    let glyph = glyph.positioned(point(0.0, 0.0));
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

        match event {
            Event::LoopDestroyed => (),
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::Resized(physical_size) => gl_context.resize(physical_size),
                _ => (),
            },
            Event::RedrawRequested(_) => {
                // generate an id for texture
                let mut texture_id = 0;
                unsafe {
                    gl::GenTextures(1, &mut texture_id);
                    gl::BindTexture(gl::TEXTURE_2D, texture_id);
                    // Set texture parameters (optional)
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
                }

                // put bitmap on texture
                unsafe {
                    gl::TexImage2D(
                        gl::TEXTURE_2D,
                        0, // Mipmap level
                        gl::RGBA as i32, // Internal format
                        width as i32,
                        height as i32,
                        0, // Border (always 0)
                        gl::RGBA, // Format of the pixel data
                        gl::UNSIGNED_BYTE, // Type of the pixel data
                        bitmap.as_ptr() as *const c_void, // Pointer to the pixel data
                    );
                    gl::GenerateMipmap(gl::TEXTURE_2D);
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
