use std::collections::HashSet;
use std::path::Path;
use std::iter::Iterator;
use std::sync::Arc;
use std::time::SystemTime;
use std::fs::{read_to_string, OpenOptions};
use std::io::Write;
use crop::Rope;
use im::OrdMap;

use arc_swap::ArcSwap;
use winit::event_loop::EventLoopProxy;
use std::sync::mpsc;

use crate::pane::Selection;
use crate::pane::Pane;
use crate::pane::PaneId;

pub type BufferId = usize;

pub enum BufferOp {
    Insert(String),
    Delete,
    Save,
    MoveHorizontal(i64),
    MoveVertical(i64),
    SetMainCursor(usize),
    AddCursor(usize),
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum CustomEvent {
    BufferRequestedRedraw(BufferId),
    CursorBlink(bool),
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub filename: Arc<Path>,
    // whether we've modified the buffer since `file_time`
    pub is_modified: bool,
    // last time that the file and buffer were identical 
    // (used to check if other processes have changed it)
    pub file_time: SystemTime,
}

#[derive(Debug, Clone)]
pub struct TextBuffer {
    pub file: Option<FileInfo>,
    pub contents: Rope,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self {
            file: None,
            contents: Rope::from(""),
        }
    }
}

impl TextBuffer {
    pub fn new(
        file: Option<FileInfo>,
        contents: Rope,
        ) -> Self { 
            Self { 
                file,
                contents,
            }
    }

    pub fn from_filename(filename_str: &str) -> Result<Self, std::io::Error> {
        let filename: Arc<Path> = Arc::from(Path::new(filename_str));
        let size = filename.metadata()?.len();
        let three_gb = 3*1024*1024*1024;
        if size >= three_gb {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "File was larger than 3GB"));
        }
        let contents = Rope::from(read_to_string(filename_str)?);
        let fi = FileInfo {filename, is_modified: false, file_time: SystemTime::now()};
        Ok(Self {file: Some(fi), contents, ..Default::default()})
    }

    pub fn from_blank() -> Self {
        let contents = Rope::new();
        Self {file: None, contents, ..Default::default()}
    }

    // has to return Self because writing updates the file info 
    // (when they were last in sync and if it's modified)
    pub fn write(&self, filename: &Path) -> Result<Self, std::io::Error> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(filename)?;
        for chunk in self.contents.chunks() {
            file.write_all(chunk.as_bytes())?;
        }

        let fi = Some(FileInfo {
                filename: Arc::from(filename),
                is_modified: false,
                file_time: SystemTime::now(),
        });
        let contents = self.contents.clone();
        Ok(Self {
            file: fi,
            contents,
            ..*self
        })
    }

    pub fn num_lines(&self) -> usize {
        self.contents.lines().count()
    }

    // lines are 0 indexed
    // end is not included
    pub fn nowrap_lines(&self, start: usize, end: usize) -> (crop::iter::Graphemes, usize) {
        let byte_len = self.contents.byte_of_line(start);
        let byte_len2 = self.contents.byte_of_line(end);
        let gs = self.contents.byte_slice(byte_len..byte_len2).graphemes();
        (gs, byte_len)
    }

    // move_vertical moves the main cursor vertically by `offset` lines
    // we use the heuristic the width is about the # of graphemes in the line
    pub fn move_vertical(&self, offset: i64, panes: Vec<Pane>, active: Vec<PaneId>) -> (Self, Vec<Pane>) {
        let mut h = HashSet::new();
        for p in panes.iter() {
            h.insert(p.buffer_id);
        }
        // assert only 1 buffer is involved (this buffer)
        assert!(h.len() == 1, "Only 1 buffer should be involved. Found: {:?}", h);

        // this can change
        assert!(active.len() == 1);
        assert!(panes.len() == 1);
        let pane = panes.iter().filter(|pane| pane.id == active[0]).nth(0).expect("the active pane must be in the involved panes");

        let file = self.file.clone();
        let mut cursors = OrdMap::new();
        let mut main_cursor_start = usize::MAX;
        for s in pane.cursors_iter() {
            let line = self.contents.line_of_byte(s.start);
            let other_line = ((line as i64 + offset).max(0) as usize).min(self.contents.line_len());
            let other_line_start = self.contents.byte_of_line(other_line);
            let other_line_end = if other_line == self.contents.line_len() {
                self.contents.byte_len()
            } else {
                self.contents.byte_of_line(other_line + 1)
            };

            let mut start = other_line_start;
            for _ in 0..pane.grapheme_col_offset {
                if start+1 >= other_line_end {
                    break;
                }
                start += 1;
                while !self.contents.is_grapheme_boundary(start) {
                    start += 1;
                }
            }
            cursors.insert(start, Selection{start, offset: 0});
            if s.start == pane.main_cursor_start {
                main_cursor_start = start;
            }
        }
        let contents = self.contents.clone();
        assert!(main_cursor_start != usize::MAX);
        
        let buf = Self {file, contents, ..*self};
        let pane = Pane {
            main_cursor_start,
            cursors,
            ..*pane
        };
        (buf, vec![pane])
    }

    // move_horizontal moves the main cursor horizontally by `offset` graphemes
    pub fn move_horizontal(&self, offset: i64, panes: Vec<Pane>, active: Vec<PaneId>) -> (Self, Vec<Pane>) {
        let mut h = HashSet::new();
        for p in panes.iter() {
            h.insert(p.buffer_id);
        }
        // assert only 1 buffer is involved (this buffer)
        assert!(h.len() == 1, "Only 1 buffer should be involved. Found: {:?}", h);

        // this can change
        assert!(active.len() == 1);
        assert!(panes.len() == 1);
        let pane = panes.iter().filter(|pane| pane.id == active[0]).nth(0).expect("the active pane must be in the involved panes");

        let file = self.file.clone();
        let mut cursors = OrdMap::new();
        let mut main_cursor_start = usize::MAX;
        let dir = offset / offset.abs();
        for s in pane.cursors_iter() {
            let mut start = s.start as i64;
            for _ in 0..offset.abs() {
                start = (start + dir).max(0).min(self.contents.byte_len() as i64);
                while !self.contents.is_grapheme_boundary(start as usize) {
                    start += dir;
                }
            }
            let start = start as usize;
            if s.start == pane.main_cursor_start {
                main_cursor_start = start;
            }
            cursors.insert(start, Selection{start, offset: 0 as i64});
        }
        let contents = self.contents.clone();
        assert!(main_cursor_start != usize::MAX);
        
        // optimization: we could try to guess from the offset, but need to know if we change lines
        let grapheme_col_offset = reset_grapheme_col_offset(&contents, main_cursor_start);
        let buf = Self {file, contents, ..*self};
        let pane = Pane {
            cursors, 
            main_cursor_start, 
            grapheme_col_offset, 
            ..*pane
        };
        (buf, vec![pane])
    }

    // note: this is a little tricky because it can change the number of cursors.
    // imagine: abc|d|e  when you backspace you get: ab|e
    pub fn backdelete_cursor(&self, panes: Vec<Pane>, active: Vec<PaneId>) -> (Self, Vec<Pane>) {
        let mut h = HashSet::new();
        for p in panes.iter() {
            h.insert(p.buffer_id);
        }
        // assert only 1 buffer is involved (this buffer)
        assert!(h.len() == 1, "Only 1 buffer should be involved. Found: {:?}", h);

        // this can change
        assert!(active.len() == 1);
        assert!(panes.len() == 1);
        let pane = panes.iter().filter(|pane| pane.id == active[0]).nth(0).expect("the active pane must be in the involved panes");

        let file = self.file.clone();

        let mut deleted_bytes = vec![];
        let mut contents = self.contents.clone();
        for cursor in pane.cursors_iter().rev() {
            // we don't want to go under 0 so we max with 1 before subtracting
            let mut start = cursor.start.max(1) - 1;
            while !contents.is_grapheme_boundary(start) {
                start -= 1;
            }
            let end = cursor.start;
            contents.delete(start..end);
            deleted_bytes.push(end - start);
        }

        // prefix sum (from the back) to get the adjustment
        for i in (0..(deleted_bytes.len() - 1)).rev() {
            deleted_bytes[i] += deleted_bytes[i+1];
        }

        let mut main_cursor_start = usize::MAX;
        let mut cursors = OrdMap::new();
        for (i, s) in pane.cursors_iter().enumerate() {
            let start = s.start - deleted_bytes[deleted_bytes.len()-1 - i];
            cursors.insert(start, Selection{start, offset: 0 as i64});
            if s.start == pane.main_cursor_start {
                main_cursor_start = start;
            }
        }
        assert!(main_cursor_start != usize::MAX);
        let grapheme_col_offset = reset_grapheme_col_offset(&self.contents, main_cursor_start);
        let buf = Self {file, contents, ..*self};
        let pane = Pane {
            cursors,
            main_cursor_start, 
            grapheme_col_offset,
            ..*pane
        };
        (buf, vec![pane])
    }

    // we're assuming text ends on a grapheme boundary
    pub fn insert(&self, text: &str, panes: Vec<Pane>, active: Vec<PaneId>) -> (Self, Vec<Pane>) {
        let mut h = HashSet::new();
        for p in panes.iter() {
            h.insert(p.buffer_id);
        }
        // assert only 1 buffer is involved (this buffer)
        assert!(h.len() == 1, "Only 1 buffer should be involved. Found: {:?}", h);

        // this can change
        assert!(active.len() == 1);
        assert!(panes.len() == 1);
        let pane = panes.iter().filter(|pane| pane.id == active[0]).nth(0).expect("the active pane must be in the involved panes");

        let file = if let Some(fi) = &self.file {
            let mut file = fi.clone();
            file.is_modified = true;
            Some(file)
        } else {
            None
        };

        let incr = text.len();
        let mut contents = self.contents.clone();
        let mut main_cursor_start = usize::MAX;
        let mut cursors = OrdMap::new();
        for (i, s) in pane.cursors_iter().enumerate() {
            let adjusted_start = s.start + incr*i;
            contents.insert(adjusted_start, text);
            let start = adjusted_start + incr;
            cursors.insert(start, Selection{start, offset: 0});
            if s.start == pane.main_cursor_start {
                main_cursor_start = start;
            }
        }
        assert!(main_cursor_start != usize::MAX);

        let grapheme_col_offset = reset_grapheme_col_offset(&contents, main_cursor_start);
        let buf = Self {file, contents, ..*self};
        let pane = Pane {
            cursors,
            main_cursor_start,
            grapheme_col_offset,
            ..*pane
        };
        (buf, vec![pane])
    }

    pub fn lines(&self) -> crop::iter::Lines {
        self.contents.lines()
    }
}

fn reset_grapheme_col_offset(contents: &Rope, start: usize) -> usize {
    let line_start = contents.byte_of_line(contents.line_of_byte(start));
    contents.byte_slice(line_start..start).graphemes().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create_buffer(s: &str, cursors: Vec<Selection>) -> (TextBuffer, Vec<Pane>) {
        let start = cursors[0].start;
        let contents = Rope::from(s);
        let grapheme_col_offset = reset_grapheme_col_offset(&contents, start);
        let panes = vec![Pane {
            cursors: cursors.into_iter().map(|s| (s.start, s)).collect(), 
            buffer_id: 0,
            main_cursor_start: start,
            grapheme_col_offset,
            id: 0,
            y_offset: 0.,
        }];
        let buffer = TextBuffer {
            file: None, 
            contents, 
            ..Default::default()
        };
        (buffer, panes)
    }

    #[test]
    fn test_text_buffer_insertion() {
        let (buffer, panes) = create_buffer("abcdefghigh", vec![Selection {start: 1, offset: 1}, Selection {start: 5, offset: 1}, Selection {start: 8, offset: 1}]);
        let (buffer, panes) = buffer.insert("xz", panes, vec![0]);

        let yee = buffer.contents.chunks().collect::<String>();

        // Assert that the buffer is created as expected
        assert_eq!(yee, "axzbcdexzfghxzigh");
        for (a, b) in panes[0].cursors_iter().zip(&[Selection{start: 3, offset: 0}, Selection{start: 9, offset: 0}, Selection{start: 14, offset: 0}]) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn test_lines() {
        let (buffer, _panes) = create_buffer("abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh", vec![Selection {start: 1, offset: 1}, Selection {start: 5, offset: 1}, Selection {start: 8, offset: 1}]);
        let v = vec!["abcdef", "jfkdsalfjads", "kadsjlfla", "alskdjflasd", "asdjkflsda", "aghigh"];

        for (a, b) in buffer.lines().zip(v) {
            assert_eq!(a, b);
        }
    }

    // TODO: a test for backward facing selections and deletion
    #[test]
    fn test_delete() {
        let cursors = vec![Selection {start: 1, offset: 0}, Selection {start: 5, offset: 0}, Selection {start: 8, offset: 0}];
        let s = "abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh";
        let (buffer, panes) = create_buffer(s, cursors);
        let (buffer, panes) = buffer.backdelete_cursor(panes, vec![0]);

        let yee = buffer.contents.chunks().collect::<String>();

        assert_eq!(yee, "bcdf\nfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh");
        for (a, b) in panes[0].cursors_iter().zip(&[Selection{start: 0, offset: 0}, Selection{start: 3, offset: 0}, Selection{start: 5, offset: 0}]) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn test_nowrap_lines() {
        let cursors = vec![Selection{start: 0, offset: 0}];
        let s = "abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh";
        let (buffer, _panes) = create_buffer(s, cursors);
        let (a, _) = buffer.nowrap_lines(0, 1);

        let s = "abcdef\n";
        for (a, b) in a.zip(s.chars()) {
            let c: Vec<char> = a.chars().collect();
            assert!(c.len() == 1);
            assert_eq!(c[0], b);
        }
    }

    #[test]
    fn test_nowrap_lines_snd() {
        let cursors = vec![Selection{start: 0, offset: 0}];
        let s = "abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh";
        let (buffer, _panes) = create_buffer(s, cursors);
        let (a, _) = buffer.nowrap_lines(1, 3);

        let s = "jfkdsalfjads\nkadsjlfla\n";
        for (a, b) in a.zip(s.chars()) {
            let c: Vec<char> = a.chars().collect();
            assert!(c.len() == 1);
            assert_eq!(c[0], b);
        }
    }

    #[test]
    fn test_nowrap_lines_end() {
        // no newline ending case
        let cursors = vec![Selection{start: 0, offset: 0}];
        let s = "abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh";
        let (buffer, _panes) = create_buffer(s, cursors);
        let (a, _) = buffer.nowrap_lines(5, 6);

        let s = "aghigh";
        for (a, b) in a.zip(s.chars()) {
            let c: Vec<char> = a.chars().collect();
            assert!(c.len() == 1);
            assert_eq!(c[0], b);
        }

        // newline ending case
        let cursors = vec![Selection{start: 0, offset: 0}];
        let s = "abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh\n";
        let (buffer, _panes) = create_buffer(s, cursors);
        let (b, _) = buffer.nowrap_lines(5, 6);

        let s = "aghigh\n";
        for (a, b) in b.zip(s.chars()) {
            let c: Vec<char> = a.chars().collect();
            assert!(c.len() == 1);
            assert_eq!(c[0], b);
        }
    }

    #[test]
    fn test_file_too_large() {
        let filename = "/Users/jason/src/benchmarks/fastest-grep/folded.txt";
        match TextBuffer::from_filename(filename) {
            Ok(_) => assert!(false),
            Err(e) => assert_eq!(e.to_string().as_str(), "File was larger than 3GB"),
        }
    }
}

pub struct SyncList<T> {
    list: ArcSwap<Vec<T>>,
}

impl<T> SyncList<T> where T: Clone {
    pub fn new() -> Self {
        Self {
            list: ArcSwap::new(Arc::new(vec![])),
        }
    }

    pub fn store(&self, index: usize, buffer: T) {
        let mut list: Vec<T> = (**self.list.load()).to_owned();
        if index == list.len() {
            list.push(buffer);
        } else {
            list[index] = buffer;
        }
        self.list.store(Arc::new(list));
    }

    pub fn get(&self) -> arc_swap::Guard<Arc<Vec<T>>> {
        self.list.load()
    }

    pub fn len(&self) -> usize {
        self.get().len()
    }
}

impl SyncList<Pane> {
    fn involved_panes(&self, buf_id: BufferId) -> Vec<Pane> {
        self.get().iter().filter(|pane| pane.buffer_id == buf_id).cloned().collect()
    }

    fn store_all(&self, new_panes: Vec<Pane>) {
        for pane in new_panes {
            self.store(pane.id, pane);
        }
    }
}

pub fn buffer_op_handler(buffer_rx: mpsc::Receiver<(BufferOp, Vec<PaneId>)>, buffers: Arc<SyncList<TextBuffer>>, panes: Arc<SyncList<Pane>>, render_tx: mpsc::Sender<CustomEvent>, event_loop_proxy: EventLoopProxy) -> impl FnOnce() {
    move || {
        while let Ok((buf_op, active_panes)) = buffer_rx.recv() {
            assert!(active_panes.len() == 1);
            let buf_id = panes.get()[active_panes[0]].buffer_id;
            match buf_op {
                BufferOp::Delete => {
                    let involved_panes = panes.involved_panes(buf_id);
                    let buffer = &buffers.get()[buf_id];
                    let (new_buffer, new_panes) = buffer.backdelete_cursor(involved_panes, active_panes);
                    panes.store_all(new_panes);
                    buffers.store(buf_id, new_buffer);
                },
                BufferOp::Insert(s) => {
                    let buffer = &buffers.get()[buf_id];
                    let involved_panes = panes.involved_panes(buf_id);
                    let (new_buffer, new_panes) = buffer.insert(&s, involved_panes, active_panes);
                    panes.store_all(new_panes);
                    buffers.store(buf_id, new_buffer);
                },
                BufferOp::MoveHorizontal(n) => {
                    let buffer = &buffers.get()[buf_id];
                    let involved_panes = panes.involved_panes(buf_id);
                    let (new_buffer, new_panes) = buffer.move_horizontal(n, involved_panes, active_panes);
                    panes.store_all(new_panes);
                    buffers.store(buf_id, new_buffer);
                },
                BufferOp::MoveVertical(n) => {
                    let buffer = buffers.get()[buf_id].clone();
                    let involved_panes = panes.involved_panes(buf_id);
                    let (new_buffer, new_panes) = buffer.move_vertical(n, involved_panes, active_panes);
                    panes.store_all(new_panes);
                    buffers.store(buf_id, new_buffer);
                },
                BufferOp::Save => {
                    let buffer = buffers.get()[buf_id].clone();
                    let filepath = &buffer.file.as_ref().unwrap().filename;
                    match buffer.write(filepath) {
                        Err(e) => log::error!("tried to save buffer, but {}", e),
                        Ok(b) => buffers.store(buf_id, b),
                    }
                },
                BufferOp::SetMainCursor(i) => { // if mouse is clicked for ex
                    assert!(active_panes.len() == 1);
                    let pane = &panes.get()[active_panes[0]];
                    let mut cursors = pane.cursors.clone();
                    let key = &pane.main_cursor_start;
                    cursors.remove(key);
                    cursors.insert(i, Selection{start: i, offset: 0});
                    let buffer = buffers.get()[buf_id].clone();
                    panes.store(pane.id, Pane {
                        main_cursor_start: i,
                        cursors,
                        grapheme_col_offset: reset_grapheme_col_offset(&buffer.contents, i),
                        ..*pane
                    });
                },
                BufferOp::AddCursor(start) => {
                    assert!(active_panes.len() == 1);
                    let pane = &panes.get()[active_panes[0]];
                    let mut cursors = pane.cursors.clone();
                    cursors.insert(start, Selection{start, offset: 0});
                    panes.store(pane.id, Pane {
                        cursors,
                        ..*pane
                    });
                },
            }
            // TODO: sketchy, we should tell the renderer which buffer to redraw
            if let Err(e) = render_tx.send(CustomEvent::BufferRequestedRedraw(buf_id)) {
                log::error!("failed to send redraw event: {}", e);
            } else {
                event_loop_proxy.wake_up();
            }
        }
    }
}
