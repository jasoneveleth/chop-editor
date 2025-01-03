use vello::peniko;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::platform::macos::WindowAttributesExtMacOS;
use winit::window::WindowAttributes;
use winit::application::ApplicationHandler;
use winit::event::{WindowEvent, DeviceEvent, DeviceId, Modifiers};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};
use anyhow;

use std::collections::HashMap;
use std::sync::Arc;

use crate::buffer::{TextBuffer, SyncList};
use crate::renderer::redraw_requested_handler;
use crate::renderer::FALLBACK_FONT_DATA;
use crate::renderer::{FontRender, Style, TITLEBAR_HEIGHT, X_PADDING, Y_PADDING, CURSOR_WIDTH, CURSOR_HEIGHT, get_font_metrics};
use crate::renderer::blink_cursor;
use crate::buffer::CustomEvent;
use crate::pane::{PaneId, Pane};

use std::ffi::CStr;
use std::num::NonZeroUsize;

use winit::event::{MouseScrollDelta, ElementState, MouseButton, ButtonSource};
use winit::window::CursorIcon;
use winit::window::Cursor;
use winit::keyboard::ModifiersKeyState;
use vello::RendererOptions;
use vello::kurbo::Rect;
use vello::util::RenderSurface;
use vello::{util::RenderContext, Renderer, Scene};

use objc2::rc::Retained;
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{NSApplication, NSApplicationDelegate};
use objc2_foundation::{NSArray, NSString, NSURL, MainThreadMarker, NSObject, NSObjectProtocol};

use std::thread;
use std::sync::mpsc;

use crate::buffer::BufferOp;
use crate::buffer::buffer_op_handler;
use crate::buffer::BufferId;
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
    pub window: Arc<dyn Window>,
    pub font_render: FontRender,
    pub scene: Scene,
    pub renderer: Renderer,
    pub render_cx: RenderContext,
    pub glyph_pos_caches: HashMap<BufferId, GlyphPosCache>,
    pub line_caches: HashMap<BufferId, LineCache>,
    pub layout: Layout,

    pub should_draw_cursor: bool,
}

pub struct Layout {
    pane_id: PaneId, // this will need to change
}

impl<'a> WindowState<'a> {
    fn new(surface: RenderSurface<'a>, window: Arc<dyn Window>, font_render: FontRender, scene: Scene, renderer: Renderer, render_cx: RenderContext) -> Self {
        let should_draw_cursor = true;
        let glyph_pos_caches = HashMap::new();
        let line_caches = HashMap::new();
        let layout = Layout { pane_id: 0};

        WindowState {
            surface,
            window,
            font_render,
            scene,
            renderer,
            render_cx,
            glyph_pos_caches,
            line_caches,
            layout,

            should_draw_cursor,
        }
    }
}

pub struct App<'a> {
    args: Args,
    windows: HashMap<WindowId, WindowState<'a>>,
    buffers: Arc<SyncList<TextBuffer>>,
    active_buffer: HashMap<WindowId, BufferId>,
    mods: Modifiers,
    buffer_tx: mpsc::Sender<(BufferOp, Vec<PaneId>)>,
    render_rx: mpsc::Receiver<CustomEvent>,
    cursor_blink_last_key: mpsc::Sender<()>,
    panes: Arc<SyncList<Pane>>,
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
    pub fn new(filename: Option<String>, event_loop_proxy: EventLoopProxy) -> Self {
        let font_size = 28.0;
        let bg_color = peniko::Color::rgb8(0xFA, 0xFA, 0xFA);
        let fg_color = peniko::Color::rgb8(0x0, 0x0, 0x0);
        let args = Args {font_size, bg_color, fg_color, font_data: FONT_DATA, filename};

        let (buffer_tx, buffer_rx) = mpsc::channel();

        let (render_tx, render_rx) = mpsc::channel();

        let buffers = Arc::new(SyncList::new());
        let panes = Arc::new(SyncList::new());

        // INVARIANT: BUFFERS SHOULD ONLY EVER BE MODIFIED (`store`d) BY THIS THREAD
        // If this is not upheld, then we have a race condition where the buffer changes
        // between the load, computation, and store, and we miss something
        let handler = buffer_op_handler(buffer_rx, buffers.clone(), panes.clone(), render_tx.clone(), event_loop_proxy.clone());
        thread::spawn(handler);

        let (cursor_blink_last_key, cursor_blink_rx) = mpsc::channel();
        thread::spawn(|| blink_cursor(render_tx, event_loop_proxy, cursor_blink_rx));

        if let Some(file_path) = &args.filename {
            if let Ok(buffer) = TextBuffer::from_filename(&file_path) {
                let buf_id = buffers.len();
                let pane_id = panes.len();
                panes.store(pane_id, Pane::new(buf_id, pane_id));
                buffers.store(buf_id, buffer);
            } else {
                log::error!("file doesn't exist {:?}", args.filename);
                std::process::exit(1);
            }
        } else {
            let buffer = TextBuffer::from_blank();
            let buf_id = buffers.len();
            let pane_id = panes.len();
            panes.store(pane_id, Pane::new(buf_id, pane_id));
            buffers.store(buf_id, buffer);
        }

        let app = App {
            args, 
            windows: HashMap::new(),
            mods: Modifiers::default(),
            buffers,
            active_buffer: HashMap::new(),
            buffer_tx,
            render_rx,
            cursor_blink_last_key,
            panes,
        };
        app
    }

    fn create_window(&mut self, event_loop: &dyn ActiveEventLoop, tab_id: Option<String>, buf_id: BufferId) -> anyhow::Result<WindowId> {
        let size = LogicalSize {width: 800, height: 600};
        let mut window_attributes = WindowAttributes::default()
            .with_surface_size(size)
            .with_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
            .with_titlebar_transparent(true);

        if let Some(tab_id) = tab_id {
            window_attributes = window_attributes.with_tabbing_identifier(&tab_id);
        }

        let window: Arc<dyn Window> = Arc::from(event_loop.create_window(window_attributes)?);

        // =============== OLD
        let mut render_cx = RenderContext::new();

        let font = peniko::Font::new(peniko::Blob::new(Arc::new(self.args.font_data)), 0);
        let fallback_font = peniko::Font::new(peniko::Blob::new(Arc::new(FALLBACK_FONT_DATA)), 0);
        let (line_height, ascent) = get_font_metrics(&font, self.args.font_size);

        let size = window.surface_size();

        let surface = pollster::block_on(render_cx.create_surface(window.clone(), size.width, size.height, Default::default())).unwrap();
        let device = &render_cx.devices[surface.dev_id].device;

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

        let color_scheme: HashMap<_,_> = [
            ("accent".to_string(),   peniko::Color::parse("#526FFF").unwrap()),
            ("fg".to_string(),       peniko::Color::parse("#383A42").unwrap()),
            ("bg".to_string(),       peniko::Color::parse("#FAFAFA").unwrap()),
            ("bg-1".to_string(),     peniko::Color::parse("#E5E5E6").unwrap()),
            ("bg-hl".to_string(),    peniko::Color::parse("#CECECE").unwrap()),
            ("mono-1".to_string(),   peniko::Color::parse("#383A42").unwrap()),
            ("mono-2".to_string(),   peniko::Color::parse("#696C77").unwrap()),
            ("mono-3".to_string(),   peniko::Color::parse("#A0A1A7").unwrap()),
            ("cyan".to_string(),     peniko::Color::parse("#0184BC").unwrap()),
            ("blue".to_string(),     peniko::Color::parse("#4078F2").unwrap()),
            ("purple".to_string(),   peniko::Color::parse("#A626A4").unwrap()),
            ("green".to_string(),    peniko::Color::parse("#50A14F").unwrap()),
            ("red-1".to_string(),    peniko::Color::parse("#E45649").unwrap()),
            ("red-2".to_string(),    peniko::Color::parse("#CA1243").unwrap()),
            ("orange-1".to_string(), peniko::Color::parse("#986801").unwrap()),
            ("orange-2".to_string(), peniko::Color::parse("#C18401").unwrap()),
            ("gray".to_string(),     peniko::Color::parse("#EDEDED").unwrap()),
            ("silver".to_string(),   peniko::Color::parse("#AAAAAA").unwrap()),
            ("black".to_string(),    peniko::Color::parse("#0F1011").unwrap()),
        ].iter().cloned().collect();

        let rust_syntax_map: HashMap<_, _> = [
            ("use", "purple"),
            ("let", "purple"),
            ("mutable_specifier", "purple"),
            ("if", "purple"),
            ("else", "purple"),
            ("loop", "purple"),
            ("break", "purple"),
            ("fn", "purple"),

            ("identifier", "white"),
            ("comment", "bg-1"),
            ("line_comment", "bg-1"),
            ("//", "bg-1"),
            ("\"", "green"),
            ("string_content", "green"),
            ("escape_sequence", "orange-1"),
            ("field_identifier", "blue"),
            ("type_identifier", "yellow"),
            ("<", "purple"),
            (">", "purple"),
            ("&", "purple"),
            ("*", "purple"),
            ("..", "purple"),
            ("=", "purple"),
            ("primitive_type", "blue"),
            ("integer_literal", "orange-1"),
        ].iter().map(|(x, y)| (x.to_string(), y.to_string())).collect();

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
            color_scheme,
            rust_syntax_map,
        };

        let font_render = FontRender {
            fallback_font,
            font,
            style,
        };
        // =============== /OLD

        let window_id = window.id();
        let window_state = WindowState::new(surface, window, font_render, scene, renderer, render_cx);
        self.active_buffer.insert(window_id, buf_id);
        self.windows.insert(window_id, window_state);

        log::info!("window created");
        Ok(window_id)
    }
}


impl<'a> ApplicationHandler for App<'a> {
    fn can_create_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        let buffer_id = 0;
        let win_id = self.create_window(_event_loop, None, buffer_id).unwrap();

        // redraw
        let window_state = self.windows.get(&win_id).expect("create_window() didn't put the window into the hashmap, should be impossible");
        window_state.window.request_redraw()
    }

    fn proxy_wake_up(&mut self, _event_loop: &dyn ActiveEventLoop) {
        while let Ok(event) = self.render_rx.try_recv() {
            // get last focused window
            let mut focused_window = None;
            for win_state in self.windows.values() {
                if win_state.window.has_focus() {
                    focused_window = Some(win_state.window.id());
                }
            }

            // both arms call for a redraw
            if let Some(window_id) = focused_window {
                let window_state = self.windows.get_mut(&window_id).unwrap();
                let redraw = |window_state: &mut WindowState| {
                    let buf_ind = *self.active_buffer.get(&window_id).unwrap();
                    let buffer_ref = &self.buffers.get()[buf_ind];
                    let pane = &self.panes.get()[window_state.layout.pane_id];
                    assert!(buf_ind == pane.buffer_id);
                    let (gpc, lc) = redraw_requested_handler(window_state, buffer_ref, pane);
                    window_state.glyph_pos_caches.insert(buf_ind, gpc);
                    window_state.line_caches.insert(buf_ind, lc);
                };
                match event {
                    CustomEvent::BufferRequestedRedraw(buf_id) => {
                        if buf_id == *self.active_buffer.get(&window_id).unwrap() {
                            redraw(window_state);
                        }
                    },
                    CustomEvent::CursorBlink(should_draw) => {
                        window_state.should_draw_cursor = should_draw;
                        redraw(window_state);
                    },
                }
            }
        }
    }

    // (unsupported on macOS/windows; relevant for web)
    fn resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        let _ = event_loop;
    }

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let mut window_state = self.windows.get_mut(&window_id).expect("recieving window event but we lost window, should be impossible");

        let buf_ind = *self.active_buffer.get(&window_id).expect("active window has no active buffer");
        let raw_buffer = &self.buffers.get()[buf_ind];

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            },
            WindowEvent::SurfaceResized(size) => {
                window_state.font_render.style.vheight = size.height as f32 - TITLEBAR_HEIGHT - Y_PADDING;
                window_state.font_render.style.vwidth = size.width as f32 - X_PADDING;
                window_state.render_cx.resize_surface(&mut window_state.surface, size.width, size.height);
                window_state.window.request_redraw();
            },
            WindowEvent::MouseWheel{delta, ..} => {
                match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        let end = (raw_buffer.num_lines()-1) as f32 * window_state.font_render.style.line_height;

                        // Adjust the scroll position based on the scroll delta
                        let pane = self.panes.get()[window_state.layout.pane_id].scroll_y(-y * 20., end);
                        self.panes.store(pane.id, pane);
                        log::warn!("we don't expect a linedelta from mouse scroll on macOS, ignoring");
                    },
                    MouseScrollDelta::PixelDelta(PhysicalPosition{x: _, y}) => {
                        let end = (raw_buffer.num_lines()-1) as f32 * window_state.font_render.style.line_height;
                        let pane = self.panes.get()[window_state.layout.pane_id].scroll_y(-y as f32, end);
                        self.panes.store(pane.id, pane);
                        window_state.window.request_redraw();
                    },
                }
            },
            WindowEvent::PointerButton { device_id: _, state, position, button, primary: _ } => {
                let top = window_state.font_render.style.voffset_y;
                if position.y < top as f64 {
                    return;
                }

                if state == ElementState::Pressed && button == ButtonSource::Mouse(MouseButton::Left) {
                    let x = position.x as f32;
                    let y = position.y as f32;

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
                    let closest_line = closest_line.expect("no lines in line cache");
                    let right_line: f32 = *closest_line;

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
                        let active = vec![window_state.layout.pane_id];
                        if self.mods.lalt_state() == ModifiersKeyState::Pressed || self.mods.ralt_state() == ModifiersKeyState::Pressed {
                            self.buffer_tx.send((BufferOp::AddCursor(*i), active)).unwrap();
                        } else {
                            self.buffer_tx.send((BufferOp::SetMainCursor(*i), active)).unwrap();
                        }
                    }
                } else {
                    log::info!("mouse input: {state:?}, {button:?}");
                }
            },
            WindowEvent::ModifiersChanged(state) => {
                self.mods = state
            },
            WindowEvent::PointerMoved { device_id: _, position, primary: _, source: _ } => {
                let top = window_state.font_render.style.voffset_y;
                if position.y <= top as f64 {
                    if position.x < 128. { // buttons
                        window_state.window.set_cursor(Cursor::Icon(CursorIcon::Default));
                    } else {
                        window_state.window.set_cursor(Cursor::Icon(CursorIcon::Grab));
                    }
                } else {
                    window_state.window.set_cursor(Cursor::Icon(CursorIcon::Text));
                }
            },
            WindowEvent::KeyboardInput{device_id: _, event, is_synthetic: _} => {
                if event.state != ElementState::Released {
                    self.cursor_blink_last_key.send(()).unwrap();
                    let pane = &self.panes.get()[window_state.layout.pane_id];
                    let (new_pane, ops) = pane.key(event.logical_key, &self.mods);
                    let should_redraw = new_pane.mode != pane.mode;
                    for op in ops {
                        if op == BufferOp::Exit {
                            event_loop.exit();
                        }
                        self.buffer_tx.send((op, vec![new_pane.id])).unwrap();
                    }
                    self.panes.store(new_pane.id, new_pane);
                    if should_redraw {
                        window_state.window.request_redraw();
                    }
                }
            },
            WindowEvent::RedrawRequested => {
                let pane = &self.panes.get()[window_state.layout.pane_id];
                let (gpc, lc) = redraw_requested_handler(&mut window_state, &raw_buffer, pane);
                window_state.glyph_pos_caches.insert(buf_ind, gpc);
                window_state.line_caches.insert(buf_ind, lc);
            },
            WindowEvent::ThemeChanged(_theme) => {
                // TODO
            }
            _ => (),
        }
    }

    fn device_event(&mut self, _event_loop: &dyn ActiveEventLoop, _device_id: Option<DeviceId>, _event: DeviceEvent) {
        // Handle window event.
    }

    // Emitted when the event loop is about to block and wait for new events.
    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        let _ = event_loop;
    }

    // (unsupported on macOS/windows; relevant for web: supposed to drop surfaces?)
    fn suspended(&mut self, event_loop: &dyn ActiveEventLoop) {
        let _ = event_loop;
    }
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

