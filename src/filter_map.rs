// This file just implements the filter_map_terminate function which 
// is a combination of filter_map and take_while. Rather than returning
// a normal option, you return an option 

use std::iter::Iterator;

pub enum FMTOption<T> {
    Some(T),
    None,
    Terminate,
}

pub struct FilterMapTerminate<I, F> {
    iter: I,
    f: F,
    terminated: bool,
}

impl<I, F> FilterMapTerminate<I, F> {
    fn new(iter: I, f: F) -> Self {
        FilterMapTerminate {
            iter,
            f,
            terminated: false,
        }
    }
}

impl<I, F, T> Iterator for FilterMapTerminate<I, F>
where
    I: Iterator,
    F: FnMut(I::Item) -> FMTOption<T>,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.terminated {
            return None;
        }

        while let Some(item) = self.iter.next() {
            match (self.f)(item) {
                FMTOption::Some(val) => return Some(val),
                FMTOption::None => continue,
                FMTOption::Terminate => {
                    self.terminated = true;
                    return None;
                }
            }
        }

        None
    }
}

pub fn filter_map_terminate<I, F, T>(iter: I, f: F) -> FilterMapTerminate<I, F>
where
    I: Iterator,
    F: FnMut(I::Item) -> FMTOption<T>,
{
    FilterMapTerminate::new(iter, f)
}

