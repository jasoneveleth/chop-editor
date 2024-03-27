use std::path::Path;
use std::iter::Iterator;
use std::sync::Arc;
use std::time::SystemTime;
use std::fs::read_to_string;
use unicode_segmentation::UnicodeSegmentation;
use unicode_segmentation::Graphemes;

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
    pub cursors: Arc<[Selection]>,
    // this is the index
    pub main_cursor: usize,
    contents: Arc<str>,
}

impl Selection {
    pub fn reverse(&self) -> Self {
        Self {start: self.end(), offset: -self.offset}
    }

    pub fn end(&self) -> usize {
        return ((self.start as i64) - self.offset) as usize
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
        let contents = Arc::from(read_to_string(filename_str)?);
        let fi = FileInfo {filename, is_modified: false, file_time: SystemTime::now()};
        let cursors = Arc::from([Selection{start: 0, offset: 0}]);
        Ok(Self {file: Some(fi), cursors, main_cursor: 0, contents})
    }

    pub fn write(&self, filename: &Path) -> Result<Self, std::io::Error> {
        std::fs::write(filename, &*self.contents)?;
        let fi = Some(FileInfo {
                filename: Arc::from(filename),
                is_modified: false,
                file_time: SystemTime::now(),
        });
        let cursors = self.cursors.clone();
        let contents = self.contents.clone();
        Ok(Self {file: fi, cursors, main_cursor: self.main_cursor, contents})
    }

    pub fn num_lines(&self) -> usize {
        self.contents.lines().count()
    }

    pub fn num_graphemes(&self) -> usize {
        let s: &str = &self.contents;
        UnicodeSegmentation::graphemes(s, true).count()
    }

    // 0 indexed
    // end is not included
    pub fn nowrap_lines(&self, start: usize, end: usize) -> Graphemes {
        let s: &str = &self.contents;
        let graphemes = UnicodeSegmentation::graphemes(s, true);
        // let mut grapheme_index = -1;
        let mut num_lines = 0;
        let mut byte_len = 0;
        for g in graphemes {
            if num_lines == start {
                break;
            } else {
                // grapheme_index += 1; // get i onto the index of g in graphemes
                byte_len += g.len();
                if g == "\n" {
                    num_lines += 1;
                }
            }
        }
        // grapheme_index += 1; // go onto the character after the newline

        // now `grapheme_index` is at the first grapheme of the start line
        // byte_len is the number of bytes up to that character
        // and num_lines = start

        let mut byte_len2 = 0;
        for byte in &self.contents.as_bytes()[byte_len..] {
            if num_lines == end {
                break;
            }
            if *byte == b'\n' {
                num_lines += 1;
            }
            byte_len2 += 1;
        }
        let byte_array = &self.contents.as_bytes()[byte_len..byte_len+byte_len2];

        // proof of safety: we are guarenteed that from newline to newline is valid utf8, if we
        // started with valid utf8
        let string = unsafe { std::str::from_utf8_unchecked(byte_array) };
        // let diff = UnicodeSegmentation::graphemes(string, true).count();
        // uncomment all code, some requeted (UnicodeSegmentation::graphemes(string, true), (grapheme_index as usize, grapheme_index as usize + diff))
        UnicodeSegmentation::graphemes(string, true)
    }

    pub fn move_horizontal(&self, offset: i64) -> Self {
        let file = if let Some(fileinfo) = &self.file {
            Some(FileInfo {
                filename: fileinfo.filename.clone(),
                is_modified: fileinfo.is_modified,
                file_time: fileinfo.file_time,
            })
        } else {
            None
        };
        let cursors: Vec<_> = self.cursors.iter().map(|s| Selection{start: ((s.start as i64 + offset).max(0) as usize).min(self.num_graphemes()-1), offset: 0 as i64}).collect();
        let cursors = Arc::from(cursors);
        let contents = self.contents.clone();

        Self {file, cursors, main_cursor: self.main_cursor, contents}
    }

    pub fn delete(&self) -> Self {
        let file = if let Some(fileinfo) = &self.file {
            Some(FileInfo {
                filename: fileinfo.filename.clone(),
                is_modified: true,
                file_time: fileinfo.file_time,
            })
        } else {
            None
        };
        let update = if self.cursors[self.main_cursor].offset == 0 {
            // adding 1 in zero case fixes it
            |(i, s): (usize, &Selection)| Selection{start: s.start + (s.start == 0) as usize - (i+1), offset: 0 as i64}
        } else {
            |(_, s): (usize, &Selection)| Selection{start: s.end(), offset: 0 as i64}
        };
        let cursors: Vec<Selection> = self.cursors.iter().enumerate().map(update).collect();
        let cursors = Arc::from(cursors);

        let mut contents = String::new();
        let mut prev = 0;
        for selection in self.cursors.iter() {
            // adding 1 in zero case fixes it
            contents += &self.contents[prev..selection.start + (selection.start==0) as usize -1];
            prev = selection.start;
        }
        contents += &self.contents[prev..];
        let contents = Arc::from(contents);

        Self {file, cursors, main_cursor: self.main_cursor, contents}
    }

    // pub fn up(&self) -> Self {
    //     let cursors: Vec<Selection> = self.cursors.iter().
    // }

    pub fn insert(&self, text: &str) -> Self {
        let file = if let Some(fileinfo) = &self.file {
            Some(FileInfo {
                filename: fileinfo.filename.clone(),
                is_modified: true,
                file_time: fileinfo.file_time,
            })
        } else {
            None
        };
        let cursors: Vec<Selection> = self.cursors.iter().enumerate().map(|(i, s)| Selection{start: s.start + (i+1) * text.len(), offset: text.len() as i64}).collect();
        let cursors = Arc::from(cursors);

        let mut contents = String::new();
        let mut prev = 0;
        for selection in self.cursors.iter() {
            contents += &self.contents[prev..selection.start];
            contents += text;
            prev = selection.start;
        }
        contents += &self.contents[prev..];
        let contents = Arc::from(contents);

        Self {file, cursors, main_cursor: self.main_cursor, contents}
    }

    pub fn lines(&self) -> std::str::Lines {
        self.contents.lines()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create_buffer(s: &str, cursors: Vec<Selection>) -> TextBuffer {
        TextBuffer {
            file: None, 
            cursors: Arc::from(cursors), 
            contents: Arc::from(s), 
            main_cursor: 0
        }
    }

    #[test]
    fn test_text_buffer_insertion() {
        let buffer = create_buffer("abcdefghigh", vec![Selection {start: 1, offset: 1}, Selection {start: 5, offset: 1}, Selection {start: 8, offset: 1}]);
        let buffer = buffer.insert("xz");

        // Assert that the buffer is created as expected
        assert_eq!(buffer.contents.as_ref(), "axzbcdexzfghxzigh");
        for (a, b) in buffer.cursors.iter().zip(&[Selection{start: 3, offset: 2}, Selection{start: 9, offset: 2}, Selection{start: 14, offset: 2}]) {
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

        assert_eq!(buffer.contents.as_ref(), "bcdf\nfkdsalfjads\nkadsjlfla\nalskdjflasd\nasdjkflsda\naghigh");
        for (a, b) in buffer.cursors.iter().zip(&[Selection{start: 0, offset: 0}, Selection{start: 3, offset: 0}, Selection{start: 5, offset: 0}]) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn test_nowrap_lines() {
        let cursors = vec![];
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
        let cursors = vec![];
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
        let cursors = vec![];
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
        let cursors = vec![];
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
        let filename = "/Users/jason/Documents/media/videos/iMovie videos/Jurasic Park - Dear Donohue.mov";
        match TextBuffer::from_filename(filename) {
            Ok(_) => assert!(false),
            Err(e) => assert_eq!(e.to_string().as_str(), "File was larger than 7GB"),
        }
    }
}
