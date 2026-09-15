//! Immutable source bytes checked against the index facts used by task queries.
use crate::index::{Index, content_hash};
use crate::output::ErrorCode;
use crate::task::Failure;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Sources {
    pub files: BTreeMap<PathBuf, Vec<u8>>,
    pub id: String,
    line_starts: BTreeMap<PathBuf, Vec<usize>>,
}
impl Sources {
    pub fn capture(index: &Index) -> Result<Self, Failure> {
        let paths = index
            .entries
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        Self::capture_subset(index, &paths)
    }
    pub fn capture_subset(
        index: &Index,
        paths: &std::collections::BTreeSet<PathBuf>,
    ) -> Result<Self, Failure> {
        if let Some(error) = &index.refresh_error {
            return Err(Failure::new(ErrorCode::RefreshFailed, error));
        }
        let mut files = BTreeMap::new();
        let mut total = 0usize;
        for path in paths {
            let Some(data) = index.entries.get(path) else {
                continue;
            };
            let (bytes, _) = crate::index::read_beneath(&index.root, path).map_err(|e| {
                Failure::new(
                    ErrorCode::AnalysisFailed,
                    format!("cannot read {}: {e}", path.display()),
                )
            })?;
            total = total.saturating_add(bytes.len());
            if total > 512 * 1024 * 1024 {
                return Err(Failure::new(
                    ErrorCode::AnalysisFailed,
                    "task source subset exceeds 512 MiB",
                ));
            }
            if content_hash(&bytes) != data.meta.content_hash {
                return Err(Failure::new(
                    ErrorCode::ContentChanged,
                    format!(
                        "{} changed after indexing; refresh and retry",
                        path.display()
                    ),
                ));
            }
            files.insert(path.clone(), bytes);
        }
        let id = Self::index_id(index);
        let mut sources = Self::new(files);
        sources.id = id;
        Ok(sources)
    }
    pub fn index_id(index: &Index) -> String {
        let mut manifest =
            format!("task-index-v2/index{}\0", crate::index::INDEX_VERSION).into_bytes();
        let mut paths: Vec<_> = index.entries.iter().collect();
        paths.sort_by_key(|(p, _)| *p);
        for (path, data) in paths {
            let name = path.to_string_lossy();
            manifest.extend_from_slice(&(name.len() as u64).to_le_bytes());
            manifest.extend_from_slice(name.as_bytes());
            manifest.extend_from_slice(&data.meta.content_hash.to_le_bytes());
        }
        format!("{:016x}", content_hash(&manifest))
    }
    pub fn index_only(index: &Index) -> Self {
        Self {
            files: BTreeMap::new(),
            id: Self::index_id(index),
            line_starts: BTreeMap::new(),
        }
    }
    pub fn new(files: BTreeMap<PathBuf, Vec<u8>>) -> Self {
        let mut manifest =
            format!("task-snapshot-v1/index{}\0", crate::index::INDEX_VERSION).into_bytes();
        for (path, bytes) in &files {
            let name = path.to_string_lossy();
            manifest.extend_from_slice(&(name.len() as u64).to_le_bytes());
            manifest.extend_from_slice(name.as_bytes());
            manifest.extend_from_slice(&content_hash(bytes).to_le_bytes());
        }
        let line_starts = files
            .iter()
            .map(|(path, bytes)| {
                let mut starts = vec![0];
                starts.extend(
                    bytes
                        .iter()
                        .enumerate()
                        .filter_map(|(i, b)| (*b == b'\n').then_some(i + 1)),
                );
                (path.clone(), starts)
            })
            .collect();
        Self {
            files,
            id: format!("{:016x}", content_hash(&manifest)),
            line_starts,
        }
    }
    pub fn check(&self, expected: Option<&str>) -> Result<(), Failure> {
        if expected.is_some_and(|id| id != self.id) {
            Err(Failure::new(
                ErrorCode::SnapshotMismatch,
                format!(
                    "snapshot changed; current identity is {}. Restart pagination",
                    self.id
                ),
            ))
        } else {
            Ok(())
        }
    }
    pub fn line(&self, path: &Path, byte: usize) -> usize {
        self.line_starts
            .get(path)
            .map_or(0, |starts| starts.partition_point(|start| *start <= byte))
    }
}

pub fn relative_file(root: &Path, path: &Path) -> Result<PathBuf, Failure> {
    let full = crate::util::path::canonical(&root.join(path));
    full.strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| Failure::new(ErrorCode::InvalidInput, "file escapes the project root"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_line_offsets_preserve_utf8_crlf_and_eof_semantics() {
        let source = Sources::new(BTreeMap::from([(
            PathBuf::from("a.rs"),
            "é\nx\r\n".as_bytes().to_vec(),
        )]));
        assert_eq!(
            [0, 2, 3, 5, 6, 999].map(|n| source.line(Path::new("a.rs"), n)),
            [1, 1, 2, 2, 3, 3]
        );
        assert_eq!(source.line(Path::new("missing"), 0), 0);
    }
}
