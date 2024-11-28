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

type BufferId = usize;

pub enum BufferOp {
    Insert(BufferId, String),
    Delete(BufferId),
    Save(BufferId),
    MoveHorizontal(BufferId, i64),
    MoveVertical(BufferId, i64),
    SetMainCursor(BufferId, usize),
    AddCursor(BufferId, usize),
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum CustomEvent {
    BufferRequestedRedraw,
    CursorBlink,
}

// let | be the cursor, and \ be the end of the selection

// "|abc\djk" start: 0, offset: 3
// "|\abcdjk" start: 0, offset: 0
// "ab\cdj|k" start: 5, offset: -3

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Selection {
    // `start` is the location of the cursor is in the selection
    // 0 means before the first byte, and n means after the nth character
    pub start: usize,
    // offset to the end of the selection such that `start + offset` is end of selection
    offset: i64,
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
    // !!! this should always be sorted
    pub cursors: OrdMap<usize, Selection>,
    // this is the index (byte offset)
    pub main_cursor_start: usize,
    // keep track so up/down will maintain rough position
    pub grapheme_col_offset: usize,
    pub contents: Rope,
}

impl Selection {
    pub fn reverse(&self) -> Self {
        Self {start: self.end(), offset: -self.offset}
    }

    pub fn end(&self) -> usize {
        return ((self.start as i64) + self.offset) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.offset == 0
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self {
            file: None,
            cursors: OrdMap::new(),
            main_cursor_start: 0,
            grapheme_col_offset: 0,
            contents: Rope::from(""),
        }
    }
}

impl TextBuffer {
    pub fn new(
        file: Option<FileInfo>,
        cursors: OrdMap<usize, Selection>,
        main_cursor_start: usize,
        grapheme_col_offset: usize,
        contents: Rope,
        ) -> Self { 
            Self { 
                file,
                cursors,
                main_cursor_start,
                grapheme_col_offset,
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
        let mut cursors: OrdMap<usize, Selection> = OrdMap::new();
        let start = 0;
        cursors.insert(start, Selection{start, offset: 0});
        Ok(Self {file: Some(fi), cursors, main_cursor_start: 0, contents, grapheme_col_offset: 0, ..Default::default()})
    }

    pub fn from_blank() -> Self {
        let contents = Rope::from("abcdefgh\nijklmnop\nqrstuvwx\nyz");
        let mut cursors = OrdMap::new();
        let start = 0;
        cursors.insert(start, Selection{start, offset: 0});
        Self {file: None, cursors, main_cursor_start: 0, contents, grapheme_col_offset: 0, ..Default::default()}
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
        let cursors = self.cursors.clone();
        let contents = self.contents.clone();
        Ok(Self {
            file: fi,
            cursors,
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
    pub fn move_vertical(&self, offset: i64) -> Self {
        let file = self.file.clone();
        let mut cursors = OrdMap::new();
        let mut main_cursor_start = usize::MAX;
        for s in self.cursors_iter() {
            let line = self.contents.line_of_byte(s.start);
            let other_line = ((line as i64 + offset).max(0) as usize).min(self.contents.line_len());
            let other_line_start = self.contents.byte_of_line(other_line);
            let other_line_end = if other_line == self.contents.line_len() {
                self.contents.byte_len()
            } else {
                self.contents.byte_of_line(other_line + 1)
            };

            let mut start = other_line_start;
            for _ in 0..self.grapheme_col_offset {
                if start+1 >= other_line_end {
                    break;
                }
                start += 1;
                while !self.contents.is_grapheme_boundary(start) {
                    start += 1;
                }
            }
            cursors.insert(start, Selection{start, offset: 0});
            if s.start == self.main_cursor_start {
                main_cursor_start = start;
            }
        }
        let contents = self.contents.clone();
        assert!(main_cursor_start != usize::MAX);
        
        Self {file, cursors, main_cursor_start, contents, ..*self}
    }

    // move_horizontal moves the main cursor horizontally by `offset` graphemes
    pub fn move_horizontal(&self, offset: i64) -> Self {
        let file = self.file.clone();
        let mut cursors = OrdMap::new();
        let mut main_cursor_start = usize::MAX;
        let dir = offset / offset.abs();
        for s in self.cursors_iter() {
            let mut start = s.start as i64;
            for _ in 0..offset.abs() {
                start = (start + dir).max(0);
                while !self.contents.is_grapheme_boundary(start as usize) {
                    start += dir;
                }
            }
            let start = (start.max(0) as usize).min(self.contents.byte_len());
            if s.start == self.main_cursor_start {
                main_cursor_start = start;
            }
            cursors.insert(start, Selection{start, offset: 0 as i64});
        }
        let contents = self.contents.clone();
        assert!(main_cursor_start != usize::MAX);
        
        let grapheme_col_offset = (self.grapheme_col_offset as i64 + offset) as usize;
        Self {file, cursors, main_cursor_start, contents, grapheme_col_offset, ..*self}
    }

    // note: this is a little tricky because it can change the number of cursors.
    // imagine: abc|d|e  when you backspace you get: ab|e
    pub fn backdelete_cursor(&self) -> Self {
        let file = self.file.clone();

        let mut deleted_bytes = vec![];
        let mut contents = self.contents.clone();
        for cursor in self.cursors_iter().rev() {
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
        for (i, s) in self.cursors_iter().enumerate() {
            let start = s.start - deleted_bytes[deleted_bytes.len()-1 - i];
            cursors.insert(start, Selection{start, offset: 0 as i64});
            if s.start == self.main_cursor_start {
                main_cursor_start = start;
            }
        }
        assert!(main_cursor_start != usize::MAX);
        let grapheme_col_offset = reset_grapheme_col_offset(&self.contents, main_cursor_start);
        Self {file, cursors, main_cursor_start, contents, grapheme_col_offset, ..*self}
    }

    // we're assuming text ends on a grapheme boundary
    pub fn insert(&self, text: &str) -> Self {
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
        for (i, s) in self.cursors_iter().enumerate() {
            let adjusted_start = s.start + incr*i;
            contents.insert(adjusted_start, text);
            let start = adjusted_start + incr;
            cursors.insert(start, Selection{start, offset: 0});
            if s.start == self.main_cursor_start {
                main_cursor_start = start;
            }
        }
        assert!(main_cursor_start != usize::MAX);

        let grapheme_col_offset = reset_grapheme_col_offset(&self.contents, main_cursor_start);
        Self {file, cursors, main_cursor_start, contents, grapheme_col_offset, ..*self}
    }

    pub fn lines(&self) -> crop::iter::Lines {
        self.contents.lines()
    }

    pub fn cursors_iter(&self) -> impl DoubleEndedIterator<Item = &Selection> {
        self.cursors.values()
    }
}

fn reset_grapheme_col_offset(contents: &Rope, start: usize) -> usize {
    contents.byte_slice(0..start).graphemes().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create_buffer(s: &str, cursors: Vec<Selection>) -> TextBuffer {
        let start = cursors[0].start;
        let contents = Rope::from(s);
        let grapheme_col_offset = reset_grapheme_col_offset(&contents, start);
        TextBuffer {
            file: None, 
            cursors: cursors.into_iter().map(|s| (s.start, s)).collect(), 
            contents, 
            main_cursor_start: start,
            grapheme_col_offset,
            ..Default::default()
        }
    }

    #[test]
    fn test_text_buffer_insertion() {
        let buffer = create_buffer("abcdefghigh", vec![Selection {start: 1, offset: 1}, Selection {start: 5, offset: 1}, Selection {start: 8, offset: 1}]);
        let buffer = buffer.insert("xz");

        let yee = buffer.contents.chunks().collect::<String>();

        // Assert that the buffer is created as expected
        assert_eq!(yee, "axzbcdexzfghxzigh");
        for (a, b) in buffer.cursors_iter().zip(&[Selection{start: 3, offset: 0}, Selection{start: 9, offset: 0}, Selection{start: 14, offset: 0}]) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn test_lines() {
        let buffer = create_buffer("abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh", vec![Selection {start: 1, offset: 1}, Selection {start: 5, offset: 1}, Selection {start: 8, offset: 1}]);
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
        let buffer = create_buffer(s, cursors);
        let buffer = buffer.backdelete_cursor();

        let yee = buffer.contents.chunks().collect::<String>();

        assert_eq!(yee, "bcdf\nfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh");
        for (a, b) in buffer.cursors_iter().zip(&[Selection{start: 0, offset: 0}, Selection{start: 3, offset: 0}, Selection{start: 5, offset: 0}]) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn test_nowrap_lines() {
        let cursors = vec![Selection{start: 0, offset: 0}];
        let s = "abcdef\njfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh";
        let buffer = create_buffer(s, cursors);
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
        let buffer = create_buffer(s, cursors);
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
        let buffer = create_buffer(s, cursors);
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
        let buffer = create_buffer(s, cursors);
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

pub struct BufferList {
    list: ArcSwap<Vec<TextBuffer>>,
}

impl BufferList {
    pub fn new() -> Self {
        Self {
            list: ArcSwap::new(Arc::new(vec![])),
        }
    }

    pub fn store(&self, index: usize, buffer: TextBuffer) {
        let mut list: Vec<TextBuffer> = (**self.list.load()).to_owned();
        if index == list.len() {
            list.push(buffer);
        } else {
            list[index] = buffer;
        }
        self.list.store(Arc::new(list));
    }

    pub fn get(&self) -> arc_swap::Guard<Arc<Vec<TextBuffer>>> {
        let x = self.list.load();
        x
    }
    
    pub fn len(&self) -> usize {
        self.get().len()
    }
}

pub fn buffer_op_handler(buffer_rx: mpsc::Receiver<BufferOp>, buffers: Arc<BufferList>, event_loop_proxy: EventLoopProxy<CustomEvent>) -> impl FnOnce() {
    move || {
        while let Ok(received) = buffer_rx.recv() {
            match received {
                BufferOp::Delete(buf_id) => {
                    buffers.store(buf_id, buffers.get()[buf_id].backdelete_cursor());
                },
                BufferOp::Insert(buf_id, s) => {
                    let buffer = &buffers.get()[buf_id];
                    buffers.store(buf_id, buffer.insert(&s));
                },
                BufferOp::MoveHorizontal(buf_id, n) => {
                    buffers.store(buf_id, buffers.get()[buf_id].move_horizontal(n));
                },
                BufferOp::MoveVertical(buf_id, n) => {
                    let buffer = buffers.get()[buf_id].clone();
                    buffers.store(buf_id, buffer.move_vertical(n));
                },
                BufferOp::Save(buf_id) => {
                    let buffer = buffers.get()[buf_id].clone();
                    let filepath = &buffer.file.as_ref().unwrap().filename;
                    match buffer.write(filepath) {
                        Err(e) => log::error!("tried to save buffer, but {}", e),
                        Ok(b) => buffers.store(buf_id, b),
                    }
                },
                BufferOp::SetMainCursor(buf_id, i) => {
                    let buffer = buffers.get()[buf_id].clone();
                    let mut cursors = buffer.cursors.clone();
                    let key = &buffer.main_cursor_start;
                    cursors.remove(key);
                    cursors.insert(i, Selection{start: i, offset: 0});
                    buffers.store(buf_id, TextBuffer {
                        main_cursor_start: i,
                        contents: buffer.contents.clone(),
                        file: buffer.file.clone(),
                        cursors,
                        grapheme_col_offset: buffer.grapheme_col_offset,
                        
                    });
                },
                BufferOp::AddCursor(buf_id, start) => {
                    let buffer = buffers.get()[buf_id].clone();
                    let mut cursors = buffer.cursors.clone();
                    cursors.insert(start, Selection{start, offset: 0});
                    buffers.store(buf_id, TextBuffer {
                        main_cursor_start: buffer.main_cursor_start,
                        contents: buffer.contents.clone(),
                        file: buffer.file.clone(),
                        cursors,
                        grapheme_col_offset: buffer.grapheme_col_offset,
                    });
                },
            }
            if let Err(e) = event_loop_proxy.send_event(CustomEvent::BufferRequestedRedraw) {
                log::error!("failed to send redraw event: {}", e);
            }
        }
    }
}
