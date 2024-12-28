use std::collections::HashMap;

use vello::peniko::Fill::NonZero;
use winit::platform::macos::WindowExtMacOS;
// use vello::glyph::skrifa::MetadataProvider;
use skrifa::MetadataProvider;
use vello::kurbo::Affine;
use vello::peniko;
use vello::skrifa;
use vello::kurbo::Rect;
use vello::Scene;
use std::sync::mpsc;

use crate::buffer::{TextBuffer, CustomEvent};
use crate::filter_map::{FMTOption, filter_map_terminate};
use crate::app::WindowState;
use crate::pane::Pane;

pub struct Style {
    pub bg_color: peniko::Color,
    pub fg_color: peniko::Color,
    pub cursor_color: peniko::Color,
    pub selection_color: peniko::Color,
    pub font_size: f32,
    pub vwidth: f32, // viewport width + height
    pub vheight: f32,
    pub voffset_x: f32, // viewport offset from top left
    pub voffset_y: f32,
    pub line_height: f32,
    pub tab_width: f32,
    pub ascent: f32,
    pub cursor_shape: Rect,
    pub titlebar: Rect,
    pub color_scheme: HashMap<String, peniko::Color>,
    pub rust_syntax_map: HashMap<String, String>,
}


pub struct FontRender {
    pub font: peniko::Font,
    pub fallback_font: peniko::Font,
    pub style: Style,
}

// don't want to write this out
pub type GlyphPosCache = HashMap<usize, ((f32, f32), (f32, f32))>;
pub type LineCache = Vec<f32>;

// const FALLBACK_FONT_DATA: &[u8] = include_bytes!("/Users/jason/Library/Fonts/NotoColorEmoji-Regular.ttf");
// const FALLBACK_FONT_DATA: &[u8] = include_bytes!("/System/Library/Fonts/Apple Color Emoji.ttc");
pub const FALLBACK_FONT_DATA: &[u8] = include_bytes!("/Users/jason/Library/Fonts/NotoEmoji-VariableFont_wght.ttf");
pub const TITLEBAR_HEIGHT: f32 = 56.;
pub const Y_PADDING: f32 = 0.0;
pub const X_PADDING: f32 = 20.0;
pub const CURSOR_WIDTH: f64 = 4.3;
pub const CURSOR_HEIGHT: f64 = 42.;

impl FontRender {
    fn render(&self, scene: &mut Scene, y_scroll: f32, buffer: &TextBuffer) -> (GlyphPosCache, LineCache) {
        log::info!("begin render");
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
        let (graphemes, mut index) = buffer.nowrap_lines(start_line as usize, last_line as usize);

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
                filter_map_terminate(graphemes, |c| {
                    let probably_one = c.len();
                    let c = c.to_string().chars().nth(0).unwrap();
                    let prev_index = index;
                    index += probably_one;
                    if c != '\n' && pen_x > self.style.vwidth { // if we're off screen just skip
                        return if line_nr >= (last_line as usize) - 1 {
                            // if we're off screen and last line we're done
                            FMTOption::Terminate
                        } else {
                            FMTOption::None
                        };
                    }

                    pos_cache.insert(prev_index, ((pen_x, pen_y), (pen_x + off_x, pen_y + off_y)));

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
                            FMTOption::Some(vello::Glyph {
                                id: gid.to_u32(),
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
                            FMTOption::Some(vello::Glyph {
                                id: gid.to_u32(),
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
                    vello::Glyph {
                        id: gid.to_u32(),
                        x,
                        y,
                    }
                }),
            );

        // add EOF glyph position
        let n = buffer.contents.byte_len();
        if n == 0 || pos_cache.get(&(n-1)).is_some() {
            pos_cache.insert(n, ((pen_x, pen_y), (pen_x + off_x, pen_y + off_y)));
        }
        (pos_cache, line_cache)
    }
}

pub fn get_font_metrics(font: &peniko::Font, font_size: f32) -> (f32, f32) {
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

pub fn redraw_requested_handler(state: &mut WindowState, buf: &TextBuffer, pane: &Pane) -> (GlyphPosCache, LineCache) {
    let renderer = &mut state.renderer;
    let scene = &mut state.scene;
    let font_render = &state.font_render;
    let width = state.surface.config.width;
    let height = state.surface.config.height;
    let frame = state.surface.surface.get_current_texture().unwrap();

    scene.reset();
    let dirty = if let Some(fi) = &buf.file {
        fi.is_modified
    } else {
        false
    };
    let (glyph_pos_cache, line_cache) = font_render.render(scene, pane.y_offset, &buf);
    for c in pane.cursors_iter() {
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
            &state.render_cx.devices[state.surface.dev_id].device,
            &state.render_cx.devices[state.surface.dev_id].queue,
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
        state.window.set_document_edited(true);
    } else {
        state.window.set_document_edited(false);
    }
    (glyph_pos_cache, line_cache)
}

pub fn blink_cursor(renderer_tx: mpsc::Sender<CustomEvent>, event_loop_proxy: winit::event_loop::EventLoopProxy, last_key: mpsc::Receiver<()>) {
    let mut last_time = std::time::Instant::now();
    let mut cursor_on = true;
    loop {
        if last_key.try_recv().is_ok() {
            last_time = std::time::Instant::now();
        }
        let should_turn_on = (last_time.elapsed().as_millis() % 1000 > 667) && cursor_on;
        let should_turn_off = (last_time.elapsed().as_millis() % 1000 <= 667) && !cursor_on;
        if should_turn_on || should_turn_off {
            cursor_on = !cursor_on;
            if renderer_tx.send(CustomEvent::CursorBlink(cursor_on)).is_err() {
                break;
            }
            event_loop_proxy.wake_up();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}


