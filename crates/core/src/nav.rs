//! Directory navigation: which images are next to the one being viewed.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::format::is_supported_extension;

/// An ordered list of images that can be stepped through.
#[derive(Debug, Clone)]
pub struct Navigator {
    directory: PathBuf,
    entries: Vec<PathBuf>,
    index: usize,
}

impl Navigator {
    /// Opens a file or a directory and selects the relevant entry.
    ///
    /// Opening a file also loads its siblings, which is what makes arrow-key
    /// browsing work after a file association double click.
    pub fn open(path: &Path) -> Result<Self> {
        if path.is_dir() {
            let entries = scan(path)?;
            return Ok(Self {
                directory: path.to_path_buf(),
                entries,
                index: 0,
            });
        }

        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

        let mut entries = scan(&parent).unwrap_or_default();
        if entries.is_empty() {
            // The parent is unreadable or holds nothing we support; still show the
            // file the user explicitly asked for.
            entries.push(path.to_path_buf());
        }
        let index = entries.iter().position(|entry| entry == path).unwrap_or(0);

        Ok(Self {
            directory: parent,
            entries,
            index,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn entries(&self) -> &[PathBuf] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn current(&self) -> Option<&Path> {
        self.entries.get(self.index).map(PathBuf::as_path)
    }

    /// Moves `delta` entries, clamped to the ends of the list.
    pub fn step(&mut self, delta: isize) -> Option<&Path> {
        let last = self.entries.len().checked_sub(1)?;
        let target = (self.index as isize + delta).clamp(0, last as isize) as usize;
        self.index = target;
        self.current()
    }

    /// Selects an entry by index.
    pub fn goto(&mut self, index: usize) -> Option<&Path> {
        if index >= self.entries.len() {
            return None;
        }
        self.index = index;
        self.current()
    }

    /// Selects an entry by path if it is part of the list.
    pub fn select(&mut self, path: &Path) -> Option<&Path> {
        let index = self.entries.iter().position(|entry| entry == path)?;
        self.goto(index)
    }

    /// The neighbouring paths, used to warm the cache before the user navigates.
    #[must_use]
    pub fn neighbours(&self, radius: usize) -> Vec<PathBuf> {
        let mut result = Vec::with_capacity(radius * 2);
        for offset in 1..=radius {
            if let Some(next) = self.entries.get(self.index + offset) {
                result.push(next.clone());
            }
            if let Some(previous) = self
                .index
                .checked_sub(offset)
                .and_then(|i| self.entries.get(i))
            {
                result.push(previous.clone());
            }
        }
        result
    }
}

fn scan(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.is_dir() {
        return Err(Error::NotADirectory {
            path: directory.to_path_buf(),
        });
    }

    let reader = std::fs::read_dir(directory).map_err(|source| Error::Io {
        path: directory.to_path_buf(),
        source,
    })?;

    let mut entries: Vec<PathBuf> = reader
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| is_supported_extension(path))
        .collect();

    entries.sort_by(|a, b| compare_paths(a, b));

    if entries.is_empty() {
        return Err(Error::EmptyDirectory {
            path: directory.to_path_buf(),
        });
    }
    Ok(entries)
}

/// Orders paths the way a human expects: `img2` before `img10`.
pub fn compare_paths(a: &Path, b: &Path) -> Ordering {
    let name_a = a
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let name_b = b
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    natural_cmp(&name_a, &name_b)
}

/// Case-insensitive natural ordering with digits compared numerically.
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    let (mut i, mut j) = (0usize, 0usize);

    while i < left.len() && j < right.len() {
        if left[i].is_ascii_digit() && right[j].is_ascii_digit() {
            let start_i = i;
            let start_j = j;
            while i < left.len() && left[i].is_ascii_digit() {
                i += 1;
            }
            while j < right.len() && right[j].is_ascii_digit() {
                j += 1;
            }
            let digits_a = strip_leading_zeros(&left[start_i..i]);
            let digits_b = strip_leading_zeros(&right[start_j..j]);
            match digits_a
                .len()
                .cmp(&digits_b.len())
                .then_with(|| digits_a.cmp(digits_b))
            {
                Ordering::Equal => {}
                other => return other,
            }
        } else {
            let fold_a = left[i].to_ascii_lowercase();
            let fold_b = right[j].to_ascii_lowercase();
            match fold_a.cmp(&fold_b) {
                Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
                other => return other,
            }
        }
    }

    (left.len() - i).cmp(&(right.len() - j))
}

fn strip_leading_zeros(digits: &[char]) -> &[char] {
    let first = digits.iter().position(|c| *c != '0');
    match first {
        Some(index) => &digits[index..],
        None => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_puts_img2_before_img10() {
        assert_eq!(natural_cmp("img2.jpg", "img10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("img10.jpg", "img9.jpg"), Ordering::Greater);
    }

    #[test]
    fn zero_padding_does_not_change_order() {
        assert_eq!(natural_cmp("img007.png", "img7.png"), Ordering::Equal);
        assert_eq!(natural_cmp("img0.png", "img0.png"), Ordering::Equal);
    }

    #[test]
    fn comparison_is_case_insensitive_but_stable() {
        assert_eq!(natural_cmp("B.png", "a.png"), Ordering::Greater);
        assert_eq!(natural_cmp("A.png", "a.png"), Ordering::Equal);
    }

    #[test]
    fn shorter_prefix_sorts_first() {
        assert_eq!(natural_cmp("a", "ab"), Ordering::Less);
        assert_eq!(natural_cmp("a.png", "ab.png"), Ordering::Less);
    }

    #[test]
    fn punctuation_orders_by_codepoint() {
        // '-' sorts before '.', so "a-1.png" precedes "a.png".
        assert_eq!(natural_cmp("a-1.png", "a.png"), Ordering::Less);
    }

    #[test]
    fn navigator_steps_and_clamps() {
        let dir = tempfile::tempdir().expect("temp dir");
        for name in ["a.png", "b.png", "c.png"] {
            std::fs::write(dir.path().join(name), b"x").expect("write");
        }
        let mut nav = Navigator::open(&dir.path().join("b.png")).expect("open");
        assert_eq!(nav.len(), 3);
        assert_eq!(nav.index(), 1);
        assert_eq!(nav.step(1).unwrap().file_name().unwrap(), "c.png");
        assert_eq!(nav.step(1).unwrap().file_name().unwrap(), "c.png");
        assert_eq!(nav.step(-5).unwrap().file_name().unwrap(), "a.png");
        assert_eq!(nav.neighbours(1).len(), 1);
    }
}
