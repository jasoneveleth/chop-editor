use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use vello::peniko::Fill::NonZero;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{WindowEvent, Modifiers};
use winit::event::{Event, MouseScrollDelta, ElementState, MouseButton};
use winit::event_loop::EventLoopBuilder;
use winit::window::CursorIcon;
use winit::window::WindowBuilder;
use winit::keyboard::{Key, NamedKey, ModifiersKeyState};
use winit::platform::macos::{WindowBuilderExtMacOS, WindowExtMacOS};
use vello::glyph::skrifa::MetadataProvider;
use vello::kurbo::Affine;
use vello::peniko;
use vello::skrifa;
use vello::util::RenderSurface;
use vello::{util::RenderContext, Renderer, RendererOptions, Scene};
use vello::kurbo::Rect;
use winit::window::Window;
use pollster::FutureExt as _;

use arc_swap::ArcSwapAny;
use crossbeam::thread;
use std::sync::mpsc;

use crate::buffer::{TextBuffer, CustomEvent};
use crate::buffer::BufferOp;
use crate::buffer::buffer_op_handler;
use crate::filter_map::{FMTOption, filter_map_terminate};

pub struct Args {
    pub font_size: f32, 
    pub bg_color: peniko::Color, 
    pub fg_color: peniko::Color,
    pub font_data: &'static [u8],
}

struct State<'s> {
    surface: RenderSurface<'s>,
    window: Arc<Window>,
    font_render: FontRender,
    scene: Scene,
    renderer: Renderer,
    render_cx: RenderContext,
    scroll_y: f32,
    should_draw_cursor: bool,
}

struct Style {
    bg_color: peniko::Color,
    fg_color: peniko::Color,
    cursor_color: peniko::Color,
    selection_color: peniko::Color,
    font_size: f32,
    vwidth: f32, // viewport width + height
    vheight: f32,
    voffset_x: f32, // viewport offset from top left
    voffset_y: f32,
    line_height: f32,
    tab_width: f32,
    ascent: f32,
    cursor_shape: Rect,
    titlebar: Rect,
}

struct FontRender {
    font: peniko::Font,
    fallback_font: peniko::Font,
    style: Style,
}

// don't want to write this out
type GlyphPosCache = HashMap<usize, ((f32, f32), (f32, f32))>;
type LineCache = Vec<f32>;

// const FALLBACK_FONT_DATA: &[u8] = include_bytes!("/Users/jason/Library/Fonts/NotoColorEmoji-Regular.ttf");
// const FALLBACK_FONT_DATA: &[u8] = include_bytes!("/System/Library/Fonts/Apple Color Emoji.ttc");
const FALLBACK_FONT_DATA: &[u8] = include_bytes!("/Users/jason/Library/Fonts/NotoEmoji-VariableFont_wght.ttf");
const TITLEBAR_HEIGHT: f32 = 56.;
const Y_PADDING: f32 = 0.0;
const X_PADDING: f32 = 20.0;
const CURSOR_WIDTH: f64 = 4.;
const CURSOR_HEIGHT: f64 = 35.;

impl FontRender {
    fn render(&self, scene: &mut Scene, y_scroll: f32, buffer: &TextBuffer) -> (GlyphPosCache, LineCache) {
        // main font
        let file_ref = skrifa::raw::FileRef::new(self.font.data.as_ref()).unwrap();
        let font_ref = match file_ref {
            skrifa::raw::FileRef::Font(f) => Some(f),
            skrifa::raw::FileRef::Collection(c) => c.get(self.font.index).ok(),
        }
        .unwrap();
        let charmap = font_ref.charmap();
        let settings: Vec<(&str, f32)> = Vec::new();
        let var_loc = font_ref.axes().location(settings.iter().copied());
        let glyph_metrics = font_ref.glyph_metrics(skrifa::instance::Size::new(self.style.font_size), &var_loc);

        // fallback
        let file_ref = skrifa::raw::FileRef::new(self.fallback_font.data.as_ref()).unwrap();
        let font_ref = match file_ref {
            skrifa::raw::FileRef::Font(f) => Some(f),
            skrifa::raw::FileRef::Collection(c) => c.get(self.fallback_font.index).ok(),
        }
        .unwrap();
        let fallback_charmap = font_ref.charmap();
        let settings: Vec<(&str, f32)> = Vec::new();
        let var_loc = font_ref.axes().location(settings.iter().copied());
        let fallback_glyph_metrics = font_ref.glyph_metrics(skrifa::instance::Size::new(self.style.font_size), &var_loc);

        let line_height = self.style.line_height;

        // y_scroll = start_line * line_height + ~~~~ means next line starts below top of screen
        let start_line = (y_scroll/line_height).floor().max(0.);
        // (line_nr)*(total_line_height) - y_offset > winow.height means line starts below bottom of screen
        let last_line = ((self.style.vheight as f32 + y_scroll)/line_height).ceil().min((buffer.num_lines()) as f32);
        let (graphemes, start_index) = buffer.nowrap_lines(start_line as usize, last_line as usize);

        // the cache of the top left corner of each glyph; specifically y=ascent, 
        // so the top of most normal capital letters
        let mut pos_cache = HashMap::new();
        // the cache of the screen height of each line (the top of the line)
        let mut line_cache = vec![];

        let mut missing = vec![];

        let mut pen_x = 0f32;
        let mut pen_y = self.style.ascent;
        let mut line_nr = start_line as usize;


        let off_x = self.style.voffset_x;
        let off_y = start_line*line_height - y_scroll + self.style.voffset_y;
        line_cache.push(pen_y + off_y);
        scene
            .draw_glyphs(&self.font)
            .font_size(self.style.font_size)
            .brush(&peniko::Brush::Solid(self.style.fg_color))
            .transform(Affine::translate((off_x as f64, off_y as f64)))
            .glyph_transform(None)
            .draw(
                NonZero,
                filter_map_terminate(graphemes.enumerate(), |(i, c)| {
                    let c = c.to_string().chars().nth(0).unwrap();
                    if c != '\n' && pen_x > self.style.vwidth { // if we're off screen just skip
                        return if line_nr >= (last_line as usize) - 1 {
                            // if we're off screen and last line we're done
                            FMTOption::Terminate
                        } else {
                            FMTOption::None
                        };
                    }

                    pos_cache.insert(i + start_index, ((pen_x, pen_y), (pen_x + off_x, pen_y + off_y)));

                    // we skip \n and \t. Otherwise try looking in the 
                    // font and the fallback font.
                    if c == '\n' {
                        line_nr += 1;
                        pen_y += line_height;
                        line_cache.push(pen_y + off_y);
                        pen_x = 0.;
                        FMTOption::None
                    } else if c == '\t' {
                        pen_x += self.style.tab_width; // TODO: should be `n*space_len`
                        FMTOption::None
                    } else {
                        let x = pen_x;
                        let y = pen_y;
                        if let Some(gid) = charmap.map(c) {
                            // if we find it in the normal font
                            pen_x += glyph_metrics.advance_width(gid).unwrap_or_default();
                            FMTOption::Some(vello::glyph::Glyph {
                                id: gid.to_u16() as u32,
                                x,
                                y,
                            })
                        } else if let Some(gid) = fallback_charmap.map(c) {
                            // if we find it in the fallback font
                            missing.push((gid, (x, y)));
                            pen_x += fallback_glyph_metrics.advance_width(gid).unwrap_or_default();
                            FMTOption::None
                        } else {
                            // if we don't find it, use the placeholder of the normal font.
                            let gid = skrifa::GlyphId::default();
                            pen_x += glyph_metrics.advance_width(gid).unwrap_or_default();
                            FMTOption::Some(vello::glyph::Glyph {
                                id: gid.to_u16() as u32,
                                x,
                                y,
                            })
                        }
                    }
                }),
            );
        // draw glyphs missing from normal font
        scene
            .draw_glyphs(&self.fallback_font)
            .font_size(self.style.font_size)
            .brush(&peniko::Brush::Solid(self.style.fg_color))
            .transform(Affine::translate((self.style.voffset_x as f64, (start_line*line_height - y_scroll + self.style.voffset_y) as f64)))
            .glyph_transform(None)
            .draw(
                NonZero,
                missing.into_iter().map(|(gid, (x, y))| {
                    vello::glyph::Glyph {
                        id: gid.to_u16() as u32,
                        x,
                        y,
                    }
                }),
            );
        (pos_cache, line_cache)
    }
}

fn super_pressed(m: &Modifiers) -> bool {
    m.lsuper_state() == ModifiersKeyState::Pressed || m.rsuper_state() == ModifiersKeyState::Pressed
}

fn get_font_metrics(font: &peniko::Font, font_size: f32) -> (f32, f32) {
    let file_ref = skrifa::raw::FileRef::new(font.data.as_ref()).unwrap();
    let font_ref = match file_ref {
        skrifa::raw::FileRef::Font(f) => Some(f),
        skrifa::raw::FileRef::Collection(c) => c.get(font.index).ok(),
    }
        .unwrap();
    let settings: Vec<(&str, f32)> = Vec::new();
    let var_loc = font_ref.axes().location(settings.iter().copied());
    let metrics = skrifa::metrics::Metrics::new(&font_ref, skrifa::instance::Size::new(font_size), &var_loc);
    let line_height = metrics.ascent + metrics.descent + metrics.leading;
    (line_height * 2., metrics.ascent)
}

fn redraw_requested_handler(state: &mut State, buffer_ref: &Arc<ArcSwapAny<Arc<TextBuffer>>>, window: &Window) -> (GlyphPosCache, LineCache) {
    let renderer = &mut state.renderer;
    let scene = &mut state.scene;
    let font_render = &state.font_render;
    let width = state.surface.config.width;
    let height = state.surface.config.height;
    let frame = state.surface.surface.get_current_texture().unwrap();

    scene.reset();
    let buf = buffer_ref.load();
    let dirty = if let Some(fi) = &buf.file {
        fi.is_modified
    } else {
        false
    };
    let (glyph_pos_cache, line_cache) = font_render.render(scene, state.scroll_y, &buf);
    for c in buf.cursors_iter() {
        if let Some(pos) = glyph_pos_cache.get(&c.start) {
            let ((_, _), (x, y)) = *pos;
            // draw cursor
            let pos = (x as f64 - CURSOR_WIDTH/2., (y - font_render.style.ascent/2.) as f64 - CURSOR_HEIGHT/2.);
            if state.should_draw_cursor {
                scene.fill(NonZero, Affine::translate(pos), font_render.style.cursor_color, None, &font_render.style.cursor_shape);
            }
            if !c.is_empty() {
                if let Some(pos) = glyph_pos_cache.get(&c.end()) {
                    let ((end_x, end_y), (_, _)) = *pos;
                    let selection = Rect::new(x.min(end_x) as f64, 0., (x - end_x).abs() as f64, font_render.style.line_height as f64);
                    assert!(end_y == y);
                    let inframe = Affine::translate((font_render.style.voffset_x as f64, font_render.style.voffset_y as f64));
                    scene.fill(NonZero, inframe, font_render.style.selection_color, None, &selection);
                }
            }
        }
    }
    // draw titlebar
    scene.fill(NonZero, Affine::IDENTITY, font_render.style.bg_color, None, &state.font_render.style.titlebar);
    renderer
        .render_to_surface(
            &state.render_cx.devices[0].device,
            &state.render_cx.devices[0].queue,
            &scene,
            &frame,
            &vello::RenderParams {
                base_color: font_render.style.bg_color,
                width,
                height,
                antialiasing_method: vello::AaConfig::Msaa16,
            },
        )
        .unwrap();
    frame.present();

    if dirty {
        (*window).set_document_edited(true);
    } else {
        (*window).set_document_edited(false);
    }
    (glyph_pos_cache, line_cache)
}

fn blink_cursor(event_loop_proxy: winit::event_loop::EventLoopProxy<CustomEvent>) {
    loop {
        event_loop_proxy.send_event(CustomEvent::CursorBlink).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

pub fn run(args: Args, buffer_ref: Arc<ArcSwapAny<Arc<TextBuffer>>>) {
    let size = LogicalSize {width: 800, height: 600};
    let mut render_cx = RenderContext::new().unwrap();

    let wb = WindowBuilder::new()
        .with_inner_size(size)
        .with_transparent(true)
        .with_titlebar_transparent(true)
        .with_fullsize_content_view(true)
        .with_title_hidden(true);

    let font = peniko::Font::new(peniko::Blob::new(Arc::new(args.font_data)), 0);
    let fallback_font = peniko::Font::new(peniko::Blob::new(Arc::new(FALLBACK_FONT_DATA)), 0);
    let (line_height, ascent) = get_font_metrics(&font, args.font_size);

    let event_loop = EventLoopBuilder::with_user_event().build().unwrap();
    let event_loop_proxy = event_loop.create_proxy();
    let event_loop_proxy2 = event_loop.create_proxy();
    let window = Arc::new(wb.build(&event_loop).unwrap());
    let size = window.inner_size();

    let surface = render_cx.create_surface(window.clone(), size.width, size.height, Default::default()).block_on().unwrap();
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
        font_size: args.font_size,
        fg_color: args.fg_color,
        bg_color: args.bg_color,
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

    let mut state = State { surface, window: window.clone(), scene, font_render, renderer, render_cx, scroll_y: 0., should_draw_cursor: true};

    let (buffer_tx, buffer_rx) = mpsc::channel();

    thread::scope(move |s| {
        // INVARIANT: `buffer_ref` SHOULD ONLY EVER BE MODIFIED (`store`d) BY THIS THREAD
        // If this is not upheld, then we have a race condition where the buffer changes
        // between the load, computation, and store, and we miss something
        s.spawn(buffer_op_handler(buffer_rx, buffer_ref.clone(), event_loop_proxy));

        // TODO: there should be a better way than spawning a thread for this
        s.spawn(move |_| { blink_cursor(event_loop_proxy2); });

        let mut mods = Modifiers::default();
        let mut glyph_pos_cache = HashMap::new();
        let mut line_cache = vec![];
        let mut curr_pos = PhysicalPosition {x: 0., y: 0.};

        event_loop.run(move |ev, elwt| match ev {
            // maybe should check that window_id of WindowEvent matches the state.window.id()
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    elwt.exit();
                },
                WindowEvent::Resized(size) => {
                    state.font_render.style.vheight = size.height as f32 - TITLEBAR_HEIGHT - Y_PADDING;
                    state.font_render.style.vwidth = size.width as f32 - X_PADDING;
                    state.render_cx.resize_surface(&mut state.surface, size.width, size.height);
                    state.window.request_redraw();
                },
                WindowEvent::MouseWheel{delta, ..} => {
                    match delta {
                        MouseScrollDelta::LineDelta(_, y) => {
                            // Adjust the scroll position based on the scroll delta
                            state.scroll_y -= y * 20.0; // Adjust the scroll speed as needed
                            log::warn!("we don't expect a linedelta from mouse scroll on macOS, ignoring");
                        },
                        MouseScrollDelta::PixelDelta(PhysicalPosition{x: _, y}) => {
                            state.scroll_y -= y as f32;
                            let real_buffer = buffer_ref.load();
                            // we want to scroll past the top (ie. negative)
                            let end = (real_buffer.num_lines()-1) as f32 * line_height;
                            state.scroll_y = state.scroll_y.max(0.).min(end);
                            state.window.request_redraw();
                        },
                    }
                },
                WindowEvent::MouseInput { device_id: _, state, button } => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        let x = curr_pos.x as f32;
                        let y = curr_pos.y as f32;

                        // find closest line
                        let mut closest_line = None;
                        let mut closest = f32::MAX;
                        for y1 in line_cache.iter() {
                            let middle = y1-ascent/2.;
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
                        for (i, ((_, _), (x1, y1))) in glyph_pos_cache.iter() {
                            if *y1 != right_line {
                                continue;
                            }
                            let dx = x - x1;
                            let dy = y - (y1-ascent/2.);

                            let dist = dx*dx + dy*dy; 
                            if dist < closest_dist {
                                closest = Some(i);
                                closest_dist = dist;
                            }
                        }
                        if let Some(i) = closest {
                            if mods.lalt_state() == ModifiersKeyState::Pressed || mods.ralt_state() == ModifiersKeyState::Pressed {
                                buffer_tx.send(BufferOp::AddCursor(*i)).unwrap();
                            } else {
                                buffer_tx.send(BufferOp::SetMainCursor(*i)).unwrap();
                            }
                        }
                    } else {
                        log::info!("mouse input: {state:?}, {button:?}");
                    }
                },
                WindowEvent::ModifiersChanged(state) => {
                    mods = state
                },
                WindowEvent::CursorMoved { device_id: _, position } => {
                    if position.y <= state.font_render.style.voffset_y as f64 {
                        state.window.set_cursor_icon(CursorIcon::Default);
                    } else {
                        state.window.set_cursor_icon(CursorIcon::Text);
                    }
                    curr_pos = position;
                },
                WindowEvent::KeyboardInput{device_id: _, event, is_synthetic: _} => {
                    if event.state != ElementState::Released {
                        match event.logical_key {
                            Key::Character(s) => {
                                // EMOJI
                                let char = s.chars().nth(0).unwrap();
                                if char == 'w' && super_pressed(&mods) {
                                    elwt.exit();
                                } else if char == 's' && super_pressed(&mods) {
                                    buffer_tx.send(BufferOp::Save).unwrap();
                                } else {
                                    buffer_tx.send(BufferOp::Insert(String::from(s.as_str()))).unwrap();
                                }
                            },
                            Key::Named(n) => {
                                match n {
                                    NamedKey::Enter => buffer_tx.send(BufferOp::Insert(String::from("\n"))).unwrap(),
                                    NamedKey::ArrowLeft => buffer_tx.send(BufferOp::MoveHorizontal(-1)).unwrap(),
                                    NamedKey::ArrowRight => buffer_tx.send(BufferOp::MoveHorizontal(1)).unwrap(),
                                    NamedKey::Space => buffer_tx.send(BufferOp::Insert(String::from(" "))).unwrap(),
                                    NamedKey::Backspace => buffer_tx.send(BufferOp::Delete).unwrap(),
                                    _ => (),
                                }
                            }
                            a => {log::info!("unknown keyboard input: {a:?}");},
                        }
                    }
                },
                WindowEvent::RedrawRequested => {
                    let (gpc, lc) = redraw_requested_handler(&mut state, &buffer_ref, &window);
                    glyph_pos_cache = gpc;
                    line_cache = lc;
                },
                _ => (),
            },
            Event::UserEvent(event) => match event {
                CustomEvent::BufferRequestedRedraw => {
                    let (gpc, lc) = redraw_requested_handler(&mut state, &buffer_ref, &window);
                    glyph_pos_cache = gpc;
                    line_cache = lc;
                },
                CustomEvent::CursorBlink => {
                    state.should_draw_cursor = !state.should_draw_cursor;
                    let (gpc, lc) = redraw_requested_handler(&mut state, &buffer_ref, &window);
                    glyph_pos_cache = gpc;
                    line_cache = lc;
                },
            },
            _ => (),
        }).unwrap();
    }).unwrap();
}

