use vello::peniko;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::platform::macos::WindowAttributesExtMacOS;
use winit::window::WindowAttributes;
use winit::application::ApplicationHandler;
use winit::event::{WindowEvent, DeviceEvent, DeviceId, Modifiers};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};
use futures::executor::block_on;

use std::collections::HashMap;
use std::sync::Arc;

use crate::buffer::{TextBuffer, BufferList};
use crate::renderer::redraw_requested_handler;
use crate::renderer::FALLBACK_FONT_DATA;
use crate::renderer::{FontRender, Style, TITLEBAR_HEIGHT, X_PADDING, Y_PADDING, CURSOR_WIDTH, CURSOR_HEIGHT, get_font_metrics};
// use crate::renderer::blink_cursor;

use std::ffi::CStr;
use std::num::NonZeroUsize;

use winit::event::{MouseScrollDelta, ElementState, MouseButton};
use winit::window::CursorIcon;
use winit::keyboard::{Key, NamedKey, ModifiersKeyState};
use vello::RendererOptions;
use vello::kurbo::Rect;
use vello::util::RenderSurface;
use vello::{util::RenderContext, Renderer, Scene};

use objc2::rc::Retained;
// use objc2::runtime::ProtocolObject;
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{NSApplication, NSApplicationDelegate};
use objc2_foundation::{NSArray, NSString, NSURL, MainThreadMarker, NSObject, NSObjectProtocol};

use std::thread;
use std::sync::mpsc;

// use crate::buffer::CustomEvent;
use crate::buffer::BufferOp;
use crate::buffer::buffer_op_handler;
use crate::buffer::CustomEvent;
use crate::renderer::{GlyphPosCache, LineCache};

const FONT_DATA: &[u8] = include_bytes!("/Users/jason/Library/Fonts/Hack-Regular.ttf");

pub struct Args {
    pub font_size: f32, 
    pub bg_color: peniko::Color, 
    pub fg_color: peniko::Color,
    pub font_data: &'static [u8],
    pub filename: Option<String>,
}

pub struct WindowState<'a> {
    pub surface: RenderSurface<'a>,
    pub window: Arc<Window>,
    pub font_render: FontRender,
    pub scene: Scene,
    pub renderer: Renderer,
    pub render_cx: RenderContext,
    pub glyph_pos_caches: HashMap<usize, GlyphPosCache>,
    pub line_caches: HashMap<usize, LineCache>,

    pub scroll_y: f32,
    pub curr_pos: PhysicalPosition<f64>,
    pub should_draw_cursor: bool,
}

impl<'a> WindowState<'a> {
    fn new(surface: RenderSurface<'a>, window: Arc<Window>, font_render: FontRender, scene: Scene, renderer: Renderer, render_cx: RenderContext) -> Self {
        let scroll_y = 0.;
        let should_draw_cursor = true;
        let glyph_pos_caches = HashMap::new();
        let line_caches = HashMap::new();
        let curr_pos = PhysicalPosition {x: 0., y: 0.};

        WindowState {
            surface,
            window,
            font_render,
            scene,
            renderer,
            render_cx,
            glyph_pos_caches,
            line_caches,

            scroll_y,
            curr_pos,
            should_draw_cursor,
        }
    }
}

pub struct App<'a> {
    args: Args,
    windows: HashMap<WindowId, WindowState<'a>>,
    buffers: Arc<BufferList>,
    active_buffer: HashMap<WindowId, usize>,
    mods: Modifiers,
    buffer_tx: mpsc::Sender<BufferOp>,
}

declare_class!(
    pub struct AppDelegate;

    unsafe impl ClassType for AppDelegate {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "MyAppDelegate";
    }

    impl DeclaredClass for AppDelegate {}

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[method(application:openURLs:)]
        #[allow(non_snake_case)]
        fn application_openURLs(&self, application: &NSApplication, urls: &NSArray<NSURL>) {
            let urls = extract_urls_from_array(urls);
            log::warn!("open urls: {application:?}, {urls:?}");
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send_id![super(mtm.alloc().set_ivars(())), init] }
    }
}

impl<'a> App<'a> {
    pub fn new(filename: Option<String>, event_loop_proxy: EventLoopProxy<CustomEvent>) -> Self {
        let font_size = 28.0;
        let bg_color = peniko::Color::rgb8(0xFA, 0xFA, 0xFA);
        let fg_color = peniko::Color::rgb8(0x0, 0x0, 0x0);
        let args = Args {font_size, bg_color, fg_color, font_data: FONT_DATA, filename};

        let (buffer_tx, buffer_rx) = mpsc::channel();

        let buffers = Arc::new(BufferList::new());

        // INVARIANT: BUFFERS SHOULD ONLY EVER BE MODIFIED (`store`d) BY THIS THREAD
        // If this is not upheld, then we have a race condition where the buffer changes
        // between the load, computation, and store, and we miss something
        let handler = buffer_op_handler(buffer_rx, buffers.clone(), event_loop_proxy);
        thread::spawn(handler);

        if let Some(file_path) = &args.filename {
            if let Ok(buffer) = TextBuffer::from_filename(&file_path) {
                buffers.store(buffers.len(), buffer);
            } else {
                log::error!("file doesn't exist {:?}", args.filename);
                std::process::exit(1);
            }
        } else {
            let buffer = TextBuffer::from_blank();
            buffers.store(buffers.len(), buffer);
        }

        let app = App {
            args, 
            windows: HashMap::new(),
            mods: Modifiers::default(),
            buffers,
            active_buffer: HashMap::new(),
            buffer_tx,
        };
        app
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop, _tab_id: Option<String>,) -> Result<(), winit::error::OsError> {
        let size = LogicalSize {width: 800, height: 600};
        let mut window_attributes = WindowAttributes::default()
            .with_inner_size(size)
            .with_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
            .with_titlebar_transparent(true);

        if let Some(tab_id) = _tab_id {
            window_attributes = window_attributes.with_tabbing_identifier(&tab_id);
        }

        let window = Arc::new(event_loop.create_window(window_attributes)?);

        // =============== OLD
        let mut render_cx = RenderContext::new();

        let font = peniko::Font::new(peniko::Blob::new(Arc::new(self.args.font_data)), 0);
        let fallback_font = peniko::Font::new(peniko::Blob::new(Arc::new(FALLBACK_FONT_DATA)), 0);
        let (line_height, ascent) = get_font_metrics(&font, self.args.font_size);

        let size = window.inner_size();

        let surface = block_on(render_cx.create_surface(window.clone(), size.width, size.height, Default::default())).unwrap();
        let device = &render_cx.devices[0].device;

        let use_cpu = false;
        let scene = Scene::new();
        let titlebar = Rect::new(0., 0., size.width as f64, TITLEBAR_HEIGHT as f64);
        let cursor_shape = Rect::new(0., 0., CURSOR_WIDTH, CURSOR_HEIGHT);
        let numthr = NonZeroUsize::new(1);
        let renderer = Renderer::new(
            &device,
            RendererOptions {
                surface_format: Some(surface.format),
                use_cpu,
                antialiasing_support: vello::AaSupport::all(),
                num_init_threads: numthr,
            },
        )
            .unwrap();

        let style = Style { 
            font_size: self.args.font_size,
            fg_color: self.args.fg_color,
            bg_color: self.args.bg_color,
            cursor_color: peniko::Color::rgb8(0x5e, 0x9c, 0xf5),
            selection_color: peniko::Color::rgba8(0x5e, 0x9c, 0xf5, 0x66),
            vheight: size.height as f32 - TITLEBAR_HEIGHT - Y_PADDING,
            vwidth: size.width as f32 - X_PADDING,
            voffset_x: X_PADDING,
            voffset_y: TITLEBAR_HEIGHT + Y_PADDING,
            tab_width: 10.,
            line_height,
            ascent,
            cursor_shape,
            titlebar,
        };

        let font_render = FontRender {
            fallback_font,
            font,
            style,
        };
        // =============== /OLD

        let window_state = WindowState::new(surface, window, font_render, scene, renderer, render_cx);
        let window_id = window_state.window.id();
        self.windows.insert(window_id, window_state);

        Ok(())
    }
}


impl<'a> ApplicationHandler<CustomEvent> for App<'a> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // you're allowed to create a window here
        self.create_window(event_loop, None).unwrap();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        // `unwrap` is fine, the window will always be available when
        // receiving a window event.
        let mut window_state = self.windows.get_mut(&window_id).unwrap();

        let buf_ind = *self.active_buffer.get(&window_id).unwrap();
        let raw_buffer = &self.buffers.get()[buf_ind];

        match event {
                WindowEvent::CloseRequested => {
                    event_loop.exit();
                },
                WindowEvent::Resized(size) => {
                    window_state.font_render.style.vheight = size.height as f32 - TITLEBAR_HEIGHT - Y_PADDING;
                    window_state.font_render.style.vwidth = size.width as f32 - X_PADDING;
                    window_state.render_cx.resize_surface(&mut window_state.surface, size.width, size.height);
                    window_state.window.request_redraw();
                },
                WindowEvent::MouseWheel{delta, ..} => {
                    match delta {
                        MouseScrollDelta::LineDelta(_, y) => {
                            // Adjust the scroll position based on the scroll delta
                            window_state.scroll_y -= y * 20.0; // Adjust the scroll speed as needed
                            log::warn!("we don't expect a linedelta from mouse scroll on macOS, ignoring");
                        },
                        MouseScrollDelta::PixelDelta(PhysicalPosition{x: _, y}) => {
                            window_state.scroll_y -= y as f32;
                            // we want to scroll past the top (ie. negative)
                            let end = (raw_buffer.num_lines()-1) as f32 * window_state.font_render.style.line_height;
                            window_state.scroll_y = window_state.scroll_y.max(0.).min(end);
                            window_state.window.request_redraw();
                        },
                    }
                },
                WindowEvent::MouseInput { device_id: _, state, button } => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        let x = window_state.curr_pos.x as f32;
                        let y = window_state.curr_pos.y as f32;

                        // find closest line
                        let mut closest_line = None;
                        let mut closest = f32::MAX;
                        let lines = window_state.line_caches.get(&buf_ind).unwrap();
                        for y1 in lines.iter() {
                            let middle = y1-window_state.font_render.style.ascent/2.;
                            let dist = (y - middle).abs();
                            if dist < closest {
                                closest = dist;
                                closest_line = Some(y1);
                            }
                        }
                        assert!(closest_line.is_some());
                        let right_line: f32 = *closest_line.unwrap();

                        // which glyph
                        let mut closest = None;
                        let mut closest_dist = f32::MAX;
                        for (i, ((_, _), (x1, y1))) in window_state.glyph_pos_caches.get(&buf_ind).unwrap().iter() {
                            if *y1 != right_line {
                                continue;
                            }
                            let dx = x - x1;
                            let dy = y - (y1-window_state.font_render.style.ascent/2.);

                            let dist = dx*dx + dy*dy; 
                            if dist < closest_dist {
                                closest = Some(i);
                                closest_dist = dist;
                            }
                        }
                        if let Some(i) = closest {
                            if self.mods.lalt_state() == ModifiersKeyState::Pressed || self.mods.ralt_state() == ModifiersKeyState::Pressed {
                                self.buffer_tx.send(BufferOp::AddCursor(buf_ind, *i)).unwrap();
                            } else {
                                self.buffer_tx.send(BufferOp::SetMainCursor(buf_ind, *i)).unwrap();
                            }
                        }
                    } else {
                        log::info!("mouse input: {state:?}, {button:?}");
                    }
                },
                WindowEvent::ModifiersChanged(state) => {
                    self.mods = state
                },
                WindowEvent::CursorMoved { device_id: _, position } => {
                    if position.y <= window_state.font_render.style.voffset_y as f64 {
                        window_state.window.set_cursor(CursorIcon::Default);
                    } else {
                        window_state.window.set_cursor(CursorIcon::Text);
                    }
                    window_state.curr_pos = position;
                },
                WindowEvent::KeyboardInput{device_id: _, event, is_synthetic: _} => {
                    log::info!("keyboard input: {:?} {:?}", event.logical_key, event.state);
                    if event.state != ElementState::Released {
                        match event.logical_key {
                            Key::Character(s) => {
                                // EMOJI
                                let char = s.chars().nth(0).unwrap();
                                if char == 'w' && super_pressed(&self.mods) {
                                    event_loop.exit();
                                } else if char == 's' && super_pressed(&self.mods) {
                                    self.buffer_tx.send(BufferOp::Save(buf_ind)).unwrap();
                                } else {
                                    self.buffer_tx.send(BufferOp::Insert(buf_ind, String::from(s.as_str()))).unwrap();
                                }
                            },
                            Key::Named(n) => {
                                match n {
                                    NamedKey::Enter => self.buffer_tx.send(BufferOp::Insert(buf_ind, String::from("\n"))).unwrap(),
                                    NamedKey::ArrowLeft => self.buffer_tx.send(BufferOp::MoveHorizontal(buf_ind, -1)).unwrap(),
                                    NamedKey::ArrowRight => self.buffer_tx.send(BufferOp::MoveHorizontal(buf_ind, 1)).unwrap(),
                                    NamedKey::ArrowUp => self.buffer_tx.send(BufferOp::MoveVertical(buf_ind, -1)).unwrap(),
                                    NamedKey::ArrowDown => self.buffer_tx.send(BufferOp::MoveVertical(buf_ind, 1)).unwrap(),
                                    NamedKey::Space => self.buffer_tx.send(BufferOp::Insert(buf_ind, String::from(" "))).unwrap(),
                                    NamedKey::Backspace => self.buffer_tx.send(BufferOp::Delete(buf_ind)).unwrap(),
                                    _ => (),
                                }
                            }
                            a => {log::info!("unknown keyboard input: {a:?}");},
                        }
                    }
                },
                WindowEvent::RedrawRequested => {
                    log::info!("redraw requested");
                    let (gpc, lc) = redraw_requested_handler(&mut window_state, &raw_buffer);
                    window_state.glyph_pos_caches.insert(buf_ind, gpc);
                    window_state.line_caches.insert(buf_ind, lc);
                },
                _ => (),
            }
    }
            // Event::UserEvent(event) => match event {
            //     CustomEvent::BufferRequestedRedraw => {
            //         let (gpc, lc) = redraw_requested_handler(&mut state, &buffer_ref, &window);
            //         glyph_pos_cache = gpc;
            //         line_cache = lc;
            //     },
            //     CustomEvent::CursorBlink => {
            //         state.should_draw_cursor = !state.should_draw_cursor;
            //         let (gpc, lc) = redraw_requested_handler(&mut state, &buffer_ref, &window);
            //         glyph_pos_cache = gpc;
            //         line_cache = lc;
            //     },
            // },
            // _ => (),

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, event: DeviceEvent) {
        // Handle window event.
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // probably remove???? idk what about_to_wait() is for
        // if let Some(window) = self.window.as_ref() {
        //     window.request_redraw();
        //     self.counter += 1;
        // }
    }
}

fn super_pressed(m: &Modifiers) -> bool {
    m.lsuper_state() == ModifiersKeyState::Pressed || m.rsuper_state() == ModifiersKeyState::Pressed
}

fn extract_urls_from_array(array: &NSArray<NSURL>) -> Vec<String> {
    let mut urls = Vec::new();

    for i in 0..array.len() {
        let url: objc2::rc::Id<NSURL> = unsafe { array.objectAtIndex(i) };

        // Convert NSURL to NSString
        let ns_string: objc2::rc::Id<NSString> = unsafe { url.absoluteString().unwrap() };

        // Convert NSString to Rust String
        let rust_string = unsafe {
            let c_str: *const std::os::raw::c_char = ns_string.UTF8String();
            CStr::from_ptr(c_str).to_string_lossy().into_owned()
        };

        urls.push(rust_string);
    }
    urls
}

