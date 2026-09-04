use std::path::{Component, Path, PathBuf};

/// Absolute, lexically normalized form of `path` (no filesystem access).
/// `.` and `..` components are folded; symlinks are left alone.
pub fn absolute_normalize(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    normalize(&abs)
}

/// The single canonical identity cx uses for a path (roadmap §4.1).
///
/// Absolute + lexically normalized + symlinks resolved, so `/tmp/p` and
/// `/private/tmp/p`, or a symlinked checkout and its target, produce one
/// identity.  Used for the project root, the index cache key, `Index.root`, and
/// every path argument, so all four agree.
///
/// Unlike [`std::fs::canonicalize`] this never fails: for a path that does not
/// exist yet, the deepest existing ancestor is canonicalized and the remaining
/// components are appended lexically.  That keeps "file not in index" errors
/// root-relative instead of leaking absolute paths.
pub fn canonical(path: &Path) -> PathBuf {
    let abs = absolute_normalize(path);
    if let Ok(resolved) = std::fs::canonicalize(&abs) {
        return strip_verbatim(&resolved);
    }

    // Walk up to the deepest existing ancestor, then re-append the tail.
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cursor = abs.as_path();
    while let (Some(parent), Some(name)) = (cursor.parent(), cursor.file_name()) {
        tail.push(name);
        if let Ok(resolved) = std::fs::canonicalize(parent) {
            let mut out = strip_verbatim(&resolved);
            out.extend(tail.iter().rev());
            return out;
        }
        cursor = parent;
    }
    abs
}

/// Drop the Windows `\\?\` verbatim prefix that `canonicalize` adds, and
/// upper-case the drive letter so `c:\p` and `C:\p` agree.  No-op elsewhere.
fn strip_verbatim(path: &Path) -> PathBuf {
    if !cfg!(windows) {
        return path.to_path_buf();
    }
    let text = path.to_string_lossy();
    let rest = text
        .strip_prefix(r"\\?\UNC\")
        .map(|unc| format!(r"\\{unc}"))
        .or_else(|| {
            text.strip_prefix(r"\\?\")
                .map(std::string::ToString::to_string)
        });
    let text = rest.unwrap_or_else(|| text.to_string());
    let mut chars: Vec<char> = text.chars().collect();
    if chars.len() >= 2 && chars[1] == ':' {
        chars[0] = chars[0].to_ascii_uppercase();
    }
    PathBuf::from(chars.into_iter().collect::<String>())
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(component.as_os_str()),
            },
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {
                out.push(component.as_os_str());
            }
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical, normalize};
    use std::path::Path;

    #[test]
    fn normalizes_parent_components() {
        assert_eq!(normalize(Path::new("/repo/child/..")), Path::new("/repo"));
        assert_eq!(
            normalize(Path::new("child/../src/lib.rs")),
            Path::new("src/lib.rs")
        );
        assert_eq!(normalize(Path::new("../src")), Path::new("../src"));
    }

    #[test]
    fn canonical_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let once = canonical(dir.path());
        assert_eq!(once, canonical(&once));
    }

    #[test]
    fn canonical_resolves_symlinked_directories() {
        let real = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(real.path().join("src")).unwrap();
        let link = home.path().join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(real.path(), &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(real.path(), &link).unwrap();

        assert_eq!(canonical(&link), canonical(real.path()));
        assert_eq!(
            canonical(&link.join("src")),
            canonical(&real.path().join("src"))
        );
    }

    #[test]
    fn canonical_handles_missing_tail_components() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let missing = canonical(&dir.path().join("src/nested/none.rs"));
        assert_eq!(missing, root.join("src/nested/none.rs"));
    }

    #[test]
    fn canonical_folds_dot_and_dotdot_against_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        let root = canonical(dir.path());
        assert_eq!(canonical(&dir.path().join(".")), root);
        assert_eq!(canonical(&dir.path().join("src/..")), root);
    }
}
