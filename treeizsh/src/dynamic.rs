//! Dynamic (filesystem-dependent) highlighting: which words name existing paths.

use std::collections::HashMap;
use std::path::Path;

/// Result of looking up a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathType {
    File,
    Directory,
}

impl PathType {
    /// The capture name used to style a word naming this kind of path.
    pub fn style(self) -> &'static str {
        match self {
            PathType::File => "path",
            PathType::Directory => "path.directory",
        }
    }
}

/// Resolves `(path, pwd)` to the kind of filesystem entry it names, if any.
pub type ResolvePath = Box<dyn Fn(&str, Option<&str>) -> Option<PathType> + Send + Sync>;

/// Memoizes path lookups so each word is `stat`ed once, not on every keystroke.
///
/// Results are keyed by the word's text and dropped whenever the working
/// directory changes (relative paths resolve differently) or on [`clear`](Self::clear).
pub struct PathCache {
    resolve: ResolvePath,
    pwd: Option<String>,
    cache: HashMap<String, Option<PathType>>,
}

impl Default for PathCache {
    fn default() -> Self {
        Self::new(Box::new(resolve_path))
    }
}

impl PathCache {
    pub fn new(resolve: ResolvePath) -> Self {
        Self {
            resolve,
            pwd: None,
            cache: HashMap::new(),
        }
    }

    /// The kind of path `text` names relative to `pwd`, or `None` if nothing
    /// exists there.
    pub fn lookup(&mut self, text: &str, pwd: Option<&str>) -> Option<PathType> {
        if text.is_empty() {
            return None;
        }
        if self.pwd.as_deref() != pwd {
            self.cache.clear();
            self.pwd = pwd.map(str::to_owned);
        }
        if let Some(&hit) = self.cache.get(text) {
            return hit;
        }
        let result = (self.resolve)(text, pwd);
        self.cache.insert(text.to_owned(), result);
        result
    }

    /// Forget every cached lookup (e.g. at the start of a new command line).
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

/// Look `path` up on the real filesystem, resolving `~` and relative paths.
pub fn resolve_path(path: &str, pwd: Option<&str>) -> Option<PathType> {
    let expanded = if path.starts_with('~') {
        if let Some(home) = std::env::var_os("HOME") {
            let home_str = home.to_string_lossy();
            if path.len() == 1 {
                home_str.into_owned()
            } else if let Some(rest) = path.strip_prefix('~').filter(|r| r.starts_with('/')) {
                format!("{home_str}{rest}")
            } else {
                path.to_string()
            }
        } else {
            path.to_string()
        }
    } else if path.starts_with('/') {
        path.to_string()
    } else if let Some(pwd) = pwd {
        // Relative path (`x`, `./x`, `../x`, `foo/bar`) — resolve against pwd.
        format!("{}/{}", pwd.trim_end_matches('/'), path)
    } else {
        path.to_string()
    };
    let p = Path::new(&expanded);
    if p.is_dir() {
        Some(PathType::Directory)
    } else if p.is_file() {
        Some(PathType::File)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counting_cache() -> (PathCache, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let cache = PathCache::new(Box::new(move |_, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            Some(PathType::File)
        }));
        (cache, calls)
    }

    #[test]
    fn lookup_is_memoized() {
        let (mut cache, calls) = counting_cache();
        for _ in 0..3 {
            assert_eq!(cache.lookup("/etc/passwd", Some("/")), Some(PathType::File));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lookup_checks_bare_words() {
        let (mut cache, calls) = counting_cache();
        assert_eq!(cache.lookup("Cargo.toml", Some("/")), Some(PathType::File));
        assert_eq!(cache.lookup("", Some("/")), None);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pwd_change_invalidates() {
        let (mut cache, calls) = counting_cache();
        cache.lookup("./x", Some("/a"));
        cache.lookup("./x", Some("/b"));
        cache.lookup("./x", Some("/b"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn clear_invalidates() {
        let (mut cache, calls) = counting_cache();
        cache.lookup("./x", Some("/a"));
        cache.clear();
        cache.lookup("./x", Some("/a"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// Regression test: bare relative paths (e.g. "foo/bar") must be resolved
    /// against the provided pwd.
    #[test]
    fn resolve_bare_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("sub");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("file.txt"), "hello").unwrap();
        let pwd = Some(dir.path().to_str().unwrap());

        assert_eq!(resolve_path("sub/file.txt", pwd), Some(PathType::File));
        assert_eq!(resolve_path("sub/", pwd), Some(PathType::Directory));
        assert_eq!(resolve_path("./sub", pwd), Some(PathType::Directory));
        assert_eq!(resolve_path("sub/nope.txt", pwd), None);
        assert_eq!(resolve_path("sub", pwd), Some(PathType::Directory));
        assert_eq!(resolve_path("nope", pwd), None);
    }
}
