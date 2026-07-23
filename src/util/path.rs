use std::path::{Component, Path, PathBuf};

pub fn absolute_normalize(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    normalize(&abs)
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
    if out.as_os_str().is_empty() { PathBuf::from(".") } else { out }
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use std::path::Path;

    #[test]
    fn normalizes_parent_components() {
        assert_eq!(normalize(Path::new("/repo/child/..")), Path::new("/repo"));
        assert_eq!(normalize(Path::new("child/../src/lib.rs")), Path::new("src/lib.rs"));
        assert_eq!(normalize(Path::new("../src")), Path::new("../src"));
    }
}
