//! Stable wire paths without changing the filesystem identity used internally.
use serde::{Serialize, Serializer};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

fn wire_text(text: &str, windows: bool) -> Cow<'_, str> {
    if windows && text.contains('\\') {
        Cow::Owned(text.replace('\\', "/"))
    } else {
        Cow::Borrowed(text)
    }
}

pub(crate) struct ProtocolPath<'a>(pub &'a Path);

impl Serialize for ProtocolPath<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let text = self
            .0
            .to_str()
            .ok_or_else(|| serde::ser::Error::custom("path contains invalid UTF-8"))?;
        serializer.serialize_str(&wire_text(text, cfg!(windows)))
    }
}

pub(crate) fn serialize_path<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    ProtocolPath(path).serialize(serializer)
}

pub(crate) fn serialize_optional_path<S: Serializer>(
    path: &Option<PathBuf>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    path.as_deref().map(ProtocolPath).serialize(serializer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_wire_paths_use_slashes_without_rewriting_unix_names() {
        assert_eq!(wire_text(r"src\é.rs", true), "src/é.rs");
        assert_eq!(wire_text(r"src\é.rs", false), r"src\é.rs");
        assert_eq!(wire_text("src/a.rs", true), "src/a.rs");
    }

    #[test]
    fn serde_paths_preserve_optional_null_and_unicode() {
        #[derive(Serialize)]
        struct Row {
            #[serde(serialize_with = "serialize_path")]
            file: PathBuf,
            #[serde(serialize_with = "serialize_optional_path")]
            before_file: Option<PathBuf>,
            #[serde(serialize_with = "serialize_optional_path")]
            after_file: Option<PathBuf>,
        }
        let file = PathBuf::from("src").join("é.rs");
        let row = Row {
            file: file.clone(),
            before_file: None,
            after_file: Some(file),
        };
        let value = serde_json::to_value(row).unwrap();
        assert_eq!(value["file"], "src/é.rs");
        assert_eq!(value["after_file"], "src/é.rs");
        assert!(value["before_file"].is_null());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_not_silently_changed() {
        use std::os::unix::ffi::OsStringExt;
        let file = PathBuf::from(std::ffi::OsString::from_vec(vec![b'x', 0xff]));
        assert!(serde_json::to_value(ProtocolPath(&file)).is_err());
    }
}
