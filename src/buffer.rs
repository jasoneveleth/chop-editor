use std::path::Path;
use std::iter::Iterator;
use std::sync::Arc;
use std::time::SystemTime;
use std::fs::{read_to_string, OpenOptions};
use std::io::Write;
use crop::Rope;
use im::OrdMap;

use arc_swap::ArcSwapAny;
use unicode_segmentation::UnicodeSegmentation;
use winit::event_loop::EventLoopProxy;
use std::sync::mpsc;

pub enum BufferOp {
    Insert(String),
    Delete,
    Save,
    MoveHorizontal(i64),
    SetMainCursor(usize),
    AddCursor(usize),
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
    // 0 means before the first character, and n means after the nth character
    // emojis count as 1
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

pub struct TextBuffer {
    pub file: Option<FileInfo>,
    // !!! this should always be sorted
    pub cursors: OrdMap<usize, Selection>,
    // this is the index
    pub main_cursor_start: usize,
    contents: Rope,
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

impl TextBuffer {
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
        Ok(Self {file: Some(fi), cursors, main_cursor_start: 0, contents})
    }

    pub fn from_blank() -> Result<Self, std::io::Error> {
        let contents = Rope::from("");
        let mut cursors = OrdMap::new();
        let start = 0;
        cursors.insert(start, Selection{start, offset: 0});
        Ok(Self {file: None, cursors, main_cursor_start: 0, contents})
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
        Ok(Self {file: fi, cursors, main_cursor_start: self.main_cursor_start, contents})
    }

    pub fn num_lines(&self) -> usize {
        self.contents.lines().count()
    }

    pub fn num_graphemes(&self) -> usize {
        return self.contents.graphemes().count();
    }

    // lines are 0 indexed
    // end is not included
    pub fn nowrap_lines(&self, start: usize, end: usize) -> (crop::iter::Graphemes, usize) {
        let byte_len = self.contents.byte_of_line(start);
        let byte_len2 = self.contents.byte_of_line(end);
        let gs = self.contents.byte_slice(byte_len..byte_len2).graphemes();

        let grapheme_index = self.contents.byte_slice(..byte_len).graphemes().count();
        (gs, grapheme_index as usize)
    }

    pub fn move_horizontal(&self, offset: i64) -> Self {
        let file = self.file.clone();
        let cursors: OrdMap<_, _> = self.cursors_iter().map(|s| {
            let start = ((s.start as i64 + offset) as usize).max(0).min(self.num_graphemes()-1);
            (start, Selection{start, offset: 0 as i64})
        }).collect();
        let contents = self.contents.clone();
        
        let main_cursor_start = ((self.main_cursor_start as i64 + offset) as usize).max(0).min(self.num_graphemes()-1);

        Self {file, cursors, main_cursor_start, contents}
    }

    pub fn delete(&self) -> Self {
        let file = self.file.clone();

        let mut contents = self.contents.clone();
        for cursor in self.cursors_iter().rev() {
            // we don't want to go under 0 so we max with 1 before subtracting
            let start = grapheme_to_byte(contents.graphemes(), cursor.start.max(1) - 1);
            let end = grapheme_to_byte(contents.graphemes(), cursor.start);
            contents.delete(start..end);
        }

        let update = if self.cursors[&self.main_cursor_start].offset == 0 {
            // adding 1 in zero case fixes it
            |(i, s): (usize, &Selection)| {
                let start = s.start + (s.start == 0) as usize - (i+1);
                (start, Selection{start, offset: 0 as i64})
            }
        } else {
            |(_, s): (usize, &Selection)| (s.end(), Selection{start: s.end(), offset: 0 as i64})
        };
        let cursors: OrdMap<_, Selection> = self.cursors_iter().enumerate().map(update).collect();

        // super awkward to keep the main_cursor in sync
        let mut main_cursor_start = usize::MAX;
        for (old, new) in self.cursors_iter().zip(cursors.values()) {
            if old.start == self.main_cursor_start {
                main_cursor_start = new.start;
                break;
            }
        }
        assert!(main_cursor_start != usize::MAX);

        Self {file, cursors, main_cursor_start, contents}
    }

    pub fn insert(&self, text: &str) -> Self {
        let file = if let Some(fi) = &self.file {
            let mut file = fi.clone();
            file.is_modified = true;
            Some(file)
        } else {
            None
        };
        let mut contents = self.contents.clone();
        let incr = UnicodeSegmentation::graphemes(text, true).count();
        let cursors: OrdMap<_, Selection> = self.cursors_iter().enumerate().map(|(i, s)| {
            let adjusted_start = s.start + incr*i;
            contents.insert(grapheme_to_byte(contents.graphemes(), adjusted_start), text);
            let start = adjusted_start + incr;
            (start, Selection{start, offset: 0})
        }).collect();

        // super awkward to keep the main_cursor in sync, since there's no easy way to 
        // know what index the main cursor is.
        let mut main_cursor_start = usize::MAX;
        for (old, new) in self.cursors_iter().zip(cursors.values()) {
            if old.start == self.main_cursor_start {
                main_cursor_start = new.start;
                break;
            }
        }
        assert!(main_cursor_start != usize::MAX);

        Self {file, cursors, main_cursor_start, contents}
    }

    pub fn lines(&self) -> crop::iter::Lines {
        self.contents.lines()
    }

    pub fn cursors_iter(&self) -> impl DoubleEndedIterator<Item = &Selection> {
        self.cursors.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create_buffer(s: &str, cursors: Vec<Selection>) -> TextBuffer {
        let start = cursors[0].start;
        TextBuffer {
            file: None, 
            cursors: cursors.into_iter().map(|s| (s.start, s)).collect(), 
            contents: Rope::from(s), 
            main_cursor_start: start,
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
        let buffer = buffer.delete();

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

fn grapheme_to_byte(graphemes: crop::iter::Graphemes, i: usize) -> usize {
    graphemes.take(i).map(|g| g.len()).sum()
}


pub fn buffer_op_handler(buffer_rx: mpsc::Receiver<BufferOp>, buffer_ref: Arc<ArcSwapAny<Arc<TextBuffer>>>, event_loop_proxy: EventLoopProxy<CustomEvent>) -> impl FnOnce(&crossbeam::thread::Scope<'_>) {
    move |_| {
        while let Ok(received) = buffer_rx.recv() {
            match received {
                BufferOp::Delete => {
                    let buffer = buffer_ref.load();
                    buffer_ref.store(Arc::new(buffer.delete()));
                },
                BufferOp::Insert(s) => {
                    let buffer = buffer_ref.load();
                    buffer_ref.store(Arc::new(buffer.insert(&s)));
                },
                BufferOp::MoveHorizontal(n) => {
                    let buffer = buffer_ref.load();
                    buffer_ref.store(Arc::new(buffer.move_horizontal(n)));
                },
                BufferOp::Save => {
                    let buffer = buffer_ref.load();
                    let filepath = &buffer.file.as_ref().unwrap().filename;
                    match buffer.write(filepath) {
                        Err(e) => log::error!("tried to save buffer, but {}", e),
                        Ok(b) => buffer_ref.store(Arc::new(b)),
                    }
                },
                BufferOp::SetMainCursor(i) => {
                    let buffer = buffer_ref.load();
                    let mut cursors = buffer.cursors.clone();
                    let key = &buffer.main_cursor_start;
                    cursors.remove(key);
                    cursors.insert(i, Selection{start: i, offset: 0});
                    buffer_ref.store(Arc::new(TextBuffer {
                        main_cursor_start: i,
                        contents: buffer.contents.clone(),
                        file: buffer.file.clone(),
                        cursors,
                    }));
                },
                BufferOp::AddCursor(start) => {
                    let buffer = buffer_ref.load();
                    let mut cursors = buffer.cursors.clone();
                    cursors.insert(start, Selection{start, offset: 0});
                    buffer_ref.store(Arc::new(TextBuffer {
                        main_cursor_start: buffer.main_cursor_start,
                        contents: buffer.contents.clone(),
                        file: buffer.file.clone(),
                        cursors,
                    }));
                },
            }
            if let Err(e) = event_loop_proxy.send_event(CustomEvent::BufferRequestedRedraw) {
                log::error!("failed to send redraw event: {}", e);
            }
        }
    }
}
