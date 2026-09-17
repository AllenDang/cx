//! Git comparisons over immutable blobs/captured working bytes. No checkout,
//! external diff, textconv, clean filter, shell interpolation or test execution.
use crate::index::{Index, Symbol};
use crate::output::ErrorCode;
use crate::snapshot::Sources;
use crate::task::{Failure, Report};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_SOURCE: usize = 8 * 1024 * 1024;
#[derive(Clone)]
pub struct Options {
    pub base: String,
    pub head: Option<String>,
    pub staged: bool,
    pub merge_base: bool,
    pub impact: bool,
    pub max_depth: usize,
    pub snapshot: Option<String>,
    pub no_tests: bool,
}
#[derive(Clone)]
struct Entry {
    mode: String,
    oid: String,
}
#[derive(Clone)]
struct Blob {
    hash: String,
    source: Option<Vec<u8>>,
    binary: bool,
    bytes: usize,
}
#[derive(Clone)]
struct File {
    mode: String,
    blob: Option<Blob>,
}
type Tree = BTreeMap<PathBuf, Entry>;
type View = BTreeMap<PathBuf, File>;

fn failure(message: impl Into<String>) -> Failure {
    Failure::new(ErrorCode::GitError, message)
}
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, Failure> {
    let out = command(root)
        .args(args)
        .output()
        .map_err(|e| failure(format!("git could not start: {e}")))?;
    if !out.status.success() {
        return Err(failure(
            String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(2000)
                .collect::<String>(),
        ));
    }
    Ok(out.stdout)
}
fn command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root).args([
        "--no-pager",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
    ]);
    command.env("GIT_TERMINAL_PROMPT", "0");
    command
}
fn commit(root: &Path, reference: &str) -> Result<String, Failure> {
    let rev = format!("{reference}^{{commit}}");
    let oid = String::from_utf8(git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &rev],
    )?)
    .map_err(|_| failure("non-UTF8 commit identity"))?;
    let oid = oid.trim().to_owned();
    if !valid_oid(&oid) {
        return Err(failure("ref did not resolve to a commit OID"));
    }
    Ok(oid)
}
fn valid_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.bytes().all(|b| b.is_ascii_hexdigit())
}
fn path(bytes: &[u8]) -> Result<PathBuf, Failure> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        failure("non-UTF8 Git paths are not supported; no lossy path merging performed")
    })?;
    let p = PathBuf::from(text);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(failure("unsafe Git path"));
    }
    Ok(p)
}
fn tree(root: &Path, oid: &str) -> Result<Tree, Failure> {
    let bytes = git(root, &["ls-tree", "-r", "-z", "--full-tree", oid])?;
    let mut out = Tree::new();
    for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let at = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(|| failure("malformed ls-tree record"))?;
        let fields = std::str::from_utf8(&record[..at])
            .map_err(|_| failure("invalid tree metadata"))?
            .split_whitespace()
            .collect::<Vec<_>>();
        if fields.len() != 3 || !valid_oid(fields[2]) {
            return Err(failure("invalid tree OID"));
        }
        out.insert(
            path(&record[at + 1..])?,
            Entry {
                mode: fields[0].into(),
                oid: fields[2].into(),
            },
        );
    }
    Ok(out)
}
fn staged_tree(bytes: &[u8]) -> Result<Tree, Failure> {
    let mut out = Tree::new();
    for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let at = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(|| failure("malformed index record"))?;
        let fields = std::str::from_utf8(&record[..at])
            .map_err(|_| failure("invalid index metadata"))?
            .split_whitespace()
            .collect::<Vec<_>>();
        if fields.len() != 3 || fields[2] != "0" || !valid_oid(fields[1]) {
            return Err(failure("unmerged or malformed Git index is not supported"));
        }
        out.insert(
            path(&record[at + 1..])?,
            Entry {
                mode: fields[0].into(),
                oid: fields[1].into(),
            },
        );
    }
    Ok(out)
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn source_wanted(path: &Path) -> bool {
    crate::language::detect_language(path).is_some()
}
fn regular(mode: &str) -> bool {
    matches!(mode, "100644" | "100755")
}

/// One cat-file process, OID-only batch input, exact byte-count framing. The
/// writer thread prevents pipe deadlock for large inventories; it runs no agent.
fn blobs(root: &Path, trees: &[&Tree], prefix: &Path) -> Result<BTreeMap<String, Blob>, Failure> {
    let mut wanted = BTreeMap::<String, bool>::new();
    for tree in trees {
        for (path, entry) in *tree {
            if path.strip_prefix(prefix).is_ok() && entry.mode != "160000" {
                *wanted.entry(entry.oid.clone()).or_default() |=
                    regular(&entry.mode) && source_wanted(path);
            }
        }
    }
    if wanted.is_empty() {
        return Ok(BTreeMap::new());
    }
    let input = wanted.keys().map(|s| format!("{s}\n")).collect::<String>();
    let mut child = command(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| failure(e.to_string()))?;
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let result = (|| {
        let mut result = BTreeMap::new();
        let mut kept = 0usize;
        for (oid, keep) in wanted {
            let mut header = String::new();
            reader
                .read_line(&mut header)
                .map_err(|e| failure(e.to_string()))?;
            let fields = header.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 3 || fields[0] != oid || fields[1] != "blob" {
                return Err(failure("invalid cat-file blob framing"));
            }
            let size: usize = fields[2]
                .parse()
                .map_err(|_| failure("invalid blob size"))?;
            let retain = keep && size <= MAX_SOURCE;
            let mut source = retain.then(|| Vec::with_capacity(size));
            let mut hasher = Sha256::new();
            let mut remaining = size;
            let mut binary = false;
            let mut buffer = [0u8; 64 * 1024];
            while remaining > 0 {
                let n = remaining.min(buffer.len());
                reader
                    .read_exact(&mut buffer[..n])
                    .map_err(|e| failure(e.to_string()))?;
                hasher.update(&buffer[..n]);
                binary |= buffer[..n].contains(&0);
                if let Some(source) = &mut source {
                    source.extend_from_slice(&buffer[..n]);
                }
                remaining -= n;
            }
            let mut newline = [0];
            reader
                .read_exact(&mut newline)
                .map_err(|e| failure(e.to_string()))?;
            if newline != *b"\n" {
                return Err(failure("invalid blob delimiter"));
            }
            if let Some(bytes) = &source {
                binary |= std::str::from_utf8(bytes).is_err();
                kept += bytes.len();
            }
            if kept > 512 * 1024 * 1024 {
                return Err(failure("Git source snapshot exceeds 512 MiB"));
            }
            result.insert(
                oid,
                Blob {
                    hash: hasher
                        .finalize()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect(),
                    source,
                    binary,
                    bytes: size,
                },
            );
        }
        Ok(result)
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    drop(reader);
    let written = writer
        .join()
        .map_err(|_| failure("Git batch writer panicked"))?
        .map_err(|e| failure(e.to_string()));
    let status = child.wait().map_err(|e| failure(e.to_string()))?;
    let result = result?;
    written?;
    if !status.success() {
        return Err(failure("git cat-file failed"));
    }
    Ok(result)
}
fn blob_view(tree: &Tree, prefix: &Path, blobs: &BTreeMap<String, Blob>) -> View {
    tree.iter()
        .filter_map(|(p, e)| {
            p.strip_prefix(prefix).ok().map(|rel| {
                (
                    rel.to_path_buf(),
                    File {
                        mode: e.mode.clone(),
                        blob: if e.mode == "160000" {
                            Some(Blob {
                                hash: e.oid.clone(),
                                source: None,
                                binary: false,
                                bytes: 0,
                            })
                        } else {
                            blobs.get(&e.oid).cloned()
                        },
                    },
                )
            })
        })
        .collect()
}
fn working_view(root: &Path, inventory: &Tree, prefix: &Path) -> Result<View, Failure> {
    let mut view = View::new();
    let mut kept = 0usize;
    let dir = cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority())
        .map_err(|e| failure(e.to_string()))?;
    for (path, entry) in inventory {
        let Ok(relative) = path.strip_prefix(prefix) else {
            continue;
        };
        if entry.mode == "160000" {
            view.insert(
                relative.into(),
                File {
                    mode: entry.mode.clone(),
                    blob: Some(Blob {
                        hash: entry.oid.clone(),
                        source: None,
                        binary: false,
                        bytes: 0,
                    }),
                },
            );
            continue;
        }
        let meta = match dir.symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(failure(e.to_string())),
        };
        let (bytes, mode) = if meta.file_type().is_symlink() {
            let link = dir
                .read_link_contents(path)
                .map_err(|e| failure(e.to_string()))?;
            (
                link.to_str()
                    .ok_or_else(|| failure("non-UTF8 symlink target"))?
                    .as_bytes()
                    .to_vec(),
                "120000".to_string(),
            )
        } else if meta.is_file() {
            let (bytes, _) = crate::index::read_beneath(root, path)
                .map_err(|e| failure(format!("cannot capture {}: {e}", path.display())))?;
            #[cfg(unix)]
            let executable = {
                use cap_std::fs::PermissionsExt;
                meta.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = entry.mode == "100755";
            (
                bytes,
                if executable { "100755" } else { "100644" }.to_string(),
            )
        } else {
            return Err(failure(format!(
                "tracked path {} is not a regular file or symlink",
                path.display()
            )));
        };
        let binary = bytes.contains(&0) || std::str::from_utf8(&bytes).is_err();
        let hash = digest(&bytes);
        let size = bytes.len();
        let source =
            (regular(&mode) && source_wanted(relative) && size <= MAX_SOURCE).then_some(bytes);
        kept += source.as_ref().map_or(0, Vec::len);
        if kept > 512 * 1024 * 1024 {
            return Err(failure("working source snapshot exceeds 512 MiB"));
        }
        view.insert(
            relative.into(),
            File {
                mode,
                blob: Some(Blob {
                    hash,
                    source,
                    binary,
                    bytes: size,
                }),
            },
        );
    }
    Ok(view)
}
fn sources(view: &View) -> Sources {
    Sources::new(
        view.iter()
            .filter_map(|(path, file)| {
                if regular(&file.mode) {
                    file.blob
                        .as_ref()?
                        .source
                        .as_ref()
                        .map(|b| (path.clone(), b.clone()))
                } else {
                    None
                }
            })
            .collect(),
    )
}
fn same(a: Option<&File>, b: Option<&File>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            a.mode == b.mode && a.blob.as_ref().map(|b| &b.hash) == b.blob.as_ref().map(|b| &b.hash)
        }
        (None, None) => true,
        _ => false,
    }
}

#[derive(Clone, Serialize)]
pub struct Hunk {
    before_start: usize,
    before_lines: usize,
    after_start: usize,
    after_lines: usize,
}
/// Bounded line LCS after trimming common prefix/suffix. A work-limit fallback
/// is explicit and never silently presented as precise hunk attribution.
fn hunks(before: &str, after: &str) -> (Vec<Hunk>, bool) {
    let a = before.split_inclusive('\n').collect::<Vec<_>>();
    let b = after.split_inclusive('\n').collect::<Vec<_>>();
    let mut prefix = 0;
    while prefix < a.len().min(b.len()) && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut n = a.len();
    let mut m = b.len();
    while n > prefix && m > prefix && a[n - 1] == b[m - 1] {
        n -= 1;
        m -= 1;
    }
    let (a, b) = (&a[prefix..n], &b[prefix..m]);
    let (n, m) = (a.len(), b.len());
    if n == 0 && m == 0 {
        return (vec![], true);
    }
    if (n + 1).saturating_mul(m + 1) > 4_000_000 {
        return (
            vec![Hunk {
                before_start: prefix + 1,
                before_lines: n,
                after_start: prefix + 1,
                after_lines: m,
            }],
            false,
        );
    }
    let width = m + 1;
    let mut dp = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * width + j] = if a[i] == b[j] {
                1 + dp[(i + 1) * width + j + 1]
            } else {
                dp[(i + 1) * width + j].max(dp[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    let mut start = None;
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            if let Some((x, y)) = start.take() {
                out.push(Hunk {
                    before_start: prefix + x + 1,
                    before_lines: i - x,
                    after_start: prefix + y + 1,
                    after_lines: j - y,
                });
            }
            i += 1;
            j += 1;
        } else {
            start.get_or_insert((i, j));
            if i < n && (j == m || dp[(i + 1) * width + j] >= dp[i * width + j + 1]) {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    if let Some((x, y)) = start {
        out.push(Hunk {
            before_start: prefix + x + 1,
            before_lines: i - x,
            after_start: prefix + y + 1,
            after_lines: j - y,
        });
    }
    (out, true)
}
#[derive(Clone, Serialize)]
struct Entity {
    name: String,
    qualified: Option<String>,
    kind: String,
    role: String,
    #[serde(serialize_with = "crate::output::serialize_path")]
    file: PathBuf,
    byte_range: (usize, usize),
    line: usize,
    end_line: usize,
    signature: String,
}
impl Entity {
    fn new(file: &Path, s: &Symbol, bytes: &[u8]) -> Self {
        let line = |n: usize| {
            1 + bytes[..n.min(bytes.len())]
                .iter()
                .filter(|b| **b == b'\n')
                .count()
        };
        Self {
            name: s.name.clone(),
            qualified: s.qualified_name.clone(),
            kind: s.kind.as_str().into(),
            role: s.role.as_str().into(),
            file: file.into(),
            byte_range: s.byte_range,
            line: line(s.byte_range.0),
            end_line: line(s.byte_range.1.saturating_sub(1)),
            signature: s.signature.clone(),
        }
    }
    fn key(&self) -> (String, String, String) {
        (
            self.qualified.clone().unwrap_or_else(|| self.name.clone()),
            self.kind.clone(),
            self.role.clone(),
        )
    }
}
#[derive(Serialize)]
struct EntityChange {
    change: &'static str,
    before: Option<Entity>,
    after: Option<Entity>,
    match_basis: &'static str,
    before_impact: Value,
    after_impact: Value,
}
#[derive(Serialize)]
pub struct Row {
    #[serde(serialize_with = "crate::output::serialize_path")]
    file: PathBuf,
    #[serde(serialize_with = "crate::output::serialize_optional_path")]
    before_file: Option<PathBuf>,
    #[serde(serialize_with = "crate::output::serialize_optional_path")]
    after_file: Option<PathBuf>,
    change: &'static str,
    classification: &'static str,
    before_mode: Option<String>,
    after_mode: Option<String>,
    before_hash: Option<String>,
    after_hash: Option<String>,
    hunks: Vec<Hunk>,
    symbols: Vec<EntityChange>,
    file_level: bool,
    complete: bool,
    warnings: Vec<String>,
}
fn rename_pairs(before: &View, after: &View) -> BTreeMap<PathBuf, PathBuf> {
    let mut old = BTreeMap::<(String, String), Vec<PathBuf>>::new();
    let mut new = BTreeMap::<(String, String), Vec<PathBuf>>::new();
    for (view, other, groups) in [(before, after, &mut old), (after, before, &mut new)] {
        for (path, file) in view {
            if !other.contains_key(path)
                && regular(&file.mode)
                && let Some(blob) = &file.blob
            {
                groups
                    .entry((file.mode.clone(), blob.hash.clone()))
                    .or_default()
                    .push(path.clone());
            }
        }
    }
    old.into_iter()
        .filter_map(|(key, paths)| {
            let added = new.get(&key)?;
            (paths.len() == 1 && added.len() == 1).then(|| (added[0].clone(), paths[0].clone()))
        })
        .collect()
}

fn intersects(e: &Entity, start: usize, count: usize) -> bool {
    if count == 0 {
        e.line < start && start <= e.end_line
    } else {
        e.line < start + count && start <= e.end_line
    }
}
fn entities(file: &Path, blob: Option<&Blob>) -> (Vec<Entity>, bool) {
    let Some(bytes) = blob.and_then(|b| b.source.as_ref()) else {
        return (vec![], true);
    };
    let Some(lang) = crate::language::detect_language(file) else {
        return (vec![], true);
    };
    match crate::language::parse_and_extract(lang, bytes, file) {
        Ok(data) => {
            let complete = if crate::language::supports_calls(lang) {
                crate::language::find_call_facts(lang, bytes, file)
                    .is_ok_and(|f| !f.has_parse_errors)
            } else {
                true
            };
            (
                data.symbols
                    .iter()
                    .map(|s| Entity::new(file, s, bytes))
                    .collect(),
                complete,
            )
        }
        Err(_) => (vec![], false),
    }
}
fn entity_changes(
    file: &Path,
    old: Option<&Blob>,
    new: Option<&Blob>,
    hunks: &[Hunk],
) -> (Vec<EntityChange>, bool, bool) {
    let (mut before, bc) = entities(file, old);
    let (mut after, ac) = entities(file, new);
    let body = |b: Option<&Blob>, e: &Entity| {
        b.and_then(|b| b.source.as_ref())
            .and_then(|s| s.get(e.byte_range.0..e.byte_range.1))
            .map(digest)
    };
    let mut changes = Vec::new();
    // Eliminate byte-identical entities first, even when preceding edits move them.
    before.retain(|a| {
        if let Some(i) = after
            .iter()
            .position(|b| a.key() == b.key() && body(old, a) == body(new, b))
        {
            after.remove(i);
            false
        } else {
            true
        }
    });
    for a in before {
        if !hunks
            .iter()
            .any(|h| intersects(&a, h.before_start, h.before_lines))
        {
            continue;
        }
        let matching: Vec<_> = after
            .iter()
            .enumerate()
            .filter(|(_, b)| b.key() == a.key())
            .map(|(i, _)| i)
            .collect();
        let b = if matching.len() == 1 {
            Some(after.remove(matching[0]))
        } else {
            None
        };
        changes.push(EntityChange {
            change: if b.is_some() { "modified" } else { "deleted" },
            before: Some(a),
            after: b,
            match_basis: if matching.len() == 1 {
                "unique_name_scope_kind; heuristic_across_versions"
            } else {
                "unmatched_old_site"
            },
            before_impact: json!({"status":"not_requested"}),
            after_impact: json!({"status":"not_requested"}),
        });
    }
    for b in after {
        if hunks
            .iter()
            .any(|h| intersects(&b, h.after_start, h.after_lines))
        {
            changes.push(EntityChange {
                change: "added",
                before: None,
                after: Some(b),
                match_basis: "unmatched_new_site",
                before_impact: json!({"status":"not_requested"}),
                after_impact: json!({"status":"not_requested"}),
            });
        }
    }
    // Don't bill an enclosing class/module/function again for changes entirely
    // contained in changed children when its own signature is unchanged.
    let keep: Vec<_> = changes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (Some(a), Some(b)) = (&c.before, &c.after) else {
                return true;
            };
            if a.signature != b.signature {
                return true;
            }
            let relevant: Vec<_> = hunks
                .iter()
                .filter(|h| {
                    intersects(a, h.before_start, h.before_lines)
                        || intersects(b, h.after_start, h.after_lines)
                })
                .collect();
            !(!relevant.is_empty()
                && relevant.iter().all(|h| {
                    changes.iter().enumerate().any(|(j, child)| {
                        if i == j {
                            return false;
                        }
                        let (Some(ca), Some(cb)) = (&child.before, &child.after) else {
                            return false;
                        };
                        a.byte_range.0 <= ca.byte_range.0
                            && ca.byte_range.1 <= a.byte_range.1
                            && a.byte_range != ca.byte_range
                            && b.byte_range.0 <= cb.byte_range.0
                            && cb.byte_range.1 <= b.byte_range.1
                            && b.byte_range != cb.byte_range
                            && (h.before_lines == 0
                                || (ca.line <= h.before_start
                                    && h.before_start + h.before_lines - 1 <= ca.end_line))
                            && (h.after_lines == 0
                                || (cb.line <= h.after_start
                                    && h.after_start + h.after_lines - 1 <= cb.end_line))
                    })
                }))
        })
        .collect();
    let changes: Vec<_> = changes
        .into_iter()
        .zip(keep)
        .filter_map(|(c, keep)| keep.then_some(c))
        .collect();
    let file_level = hunks.iter().any(|h| {
        !changes.iter().any(|c| {
            (h.before_lines == 0
                || c.before.as_ref().is_some_and(|e| {
                    e.line <= h.before_start && h.before_start + h.before_lines - 1 <= e.end_line
                }))
                && (h.after_lines == 0
                    || c.after.as_ref().is_some_and(|e| {
                        e.line <= h.after_start && h.after_start + h.after_lines - 1 <= e.end_line
                    }))
        })
    });
    (changes, bc && ac, file_level)
}
fn attach_impact(
    graph: &mut crate::impact::Graph<'_>,
    entity: Option<&Entity>,
    depth: usize,
) -> Value {
    let Some(e) = entity else {
        return json!({"status":"not_applicable"});
    };
    if e.kind != "fn" {
        return json!({"status":"unsupported_entity_kind"});
    };
    let report = graph.impact(&crate::impact::Options {
        name: e.name.clone(),
        scope: e.qualified.clone(),
        file: Some(e.file.clone()),
        line: None,
        byte_offset: Some(e.byte_range.0),
        max_depth: depth,
        max_nodes: 200,
        max_edges: 20000,
        snapshot: None,
        no_tests: false,
    });
    json!({"status":if report.error.is_some(){"unavailable"}else if report.complete{"analyzed"}else{"partial"},"results":report.rows,"analysis":report.analysis,"warnings":report.warnings,"error":report.error.map(|e|json!({"code":e.code,"message":e.message}))})
}

pub fn run(root: &Path, opts: &Options) -> Result<(Report<Row>, crate::index::Freshness), Failure> {
    let repo = String::from_utf8(git(root, &["rev-parse", "--show-toplevel"])?)
        .map_err(|_| failure("non-UTF8 repository root"))?;
    let repo = crate::util::path::canonical(Path::new(repo.trim_end_matches('\n')));
    let root = crate::util::path::canonical(root);
    let prefix = root
        .strip_prefix(&repo)
        .map_err(|_| failure("root outside Git repository"))?;
    let mut base = commit(&repo, &opts.base)?;
    let head = opts.head.as_deref().map(|r| commit(&repo, r)).transpose()?;
    if opts.merge_base {
        let h = head
            .as_ref()
            .ok_or_else(|| failure("merge-base requires --head"))?;
        base = String::from_utf8(git(&repo, &["merge-base", &base, h])?)
            .map_err(|_| failure("invalid merge-base"))?
            .trim()
            .to_owned();
        if !valid_oid(&base) {
            return Err(failure("no unique merge-base"));
        }
    }
    let before_tree = tree(&repo, &base)?;
    let index_bytes = if head.is_none() {
        Some(git(&repo, &["ls-files", "--stage", "-z"])?)
    } else {
        None
    };
    let after_tree = if let Some(h) = &head {
        tree(&repo, h)?
    } else {
        staged_tree(index_bytes.as_ref().unwrap())?
    };
    let blobs = blobs(
        &repo,
        if opts.staged || head.is_some() {
            vec![&before_tree, &after_tree]
        } else {
            vec![&before_tree]
        }
        .as_slice(),
        prefix,
    )?;
    let before = blob_view(&before_tree, prefix, &blobs);
    let after = if opts.staged || head.is_some() {
        blob_view(&after_tree, prefix, &blobs)
    } else {
        working_view(&repo, &after_tree, prefix)?
    };
    if let Some(initial) = index_bytes
        && git(&repo, &["ls-files", "--stage", "-z"])? != initial
    {
        return Err(failure("Git index changed during capture; retry"));
    }
    let paths: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
    let renames = rename_pairs(&before, &after);
    let moved_from: BTreeSet<_> = renames.values().cloned().collect();
    let mut manifest = base.clone();
    manifest.push_str(&format!("/scope:{prefix:?}/"));
    manifest.push_str(
        head.as_deref()
            .unwrap_or(if opts.staged { "index" } else { "worktree" }),
    );
    for (path, file) in &after {
        manifest.push_str(&format!(
            "{:?}:{}:{};",
            path,
            file.mode,
            file.blob.as_ref().map_or("", |b| b.hash.as_str())
        ));
    }
    let snapshot = digest(manifest.as_bytes());
    if opts.snapshot.as_deref().is_some_and(|s| s != snapshot) {
        return Err(Failure::new(
            ErrorCode::SnapshotMismatch,
            "comparison snapshot changed; restart pagination",
        ));
    }
    let mut rows = Vec::new();
    let mut complete = true;
    let mut filtered_tests = 0;
    for file in paths {
        if moved_from.contains(&file) {
            continue;
        }
        if let Some(old_path) = renames.get(&file) {
            if opts.no_tests
                && (crate::query::is_test_file(&file) || crate::query::is_test_file(old_path))
            {
                filtered_tests += 1;
                continue;
            }
            let a = &before[old_path];
            let b = &after[&file];
            let old = a.blob.as_ref();
            let new = b.blob.as_ref();
            let (old_entities, oc) = entities(old_path, old);
            let (mut new_entities, nc) = entities(&file, new);
            let mut symbols = Vec::new();
            for old_entity in old_entities {
                if let Some(n) = new_entities.iter().position(|e| {
                    e.key() == old_entity.key() && e.byte_range == old_entity.byte_range
                }) {
                    symbols.push(EntityChange {
                        change: "moved",
                        before: Some(old_entity),
                        after: Some(new_entities.remove(n)),
                        match_basis: "identical_file_content; heuristic_move",
                        before_impact: json!({"status":"not_requested"}),
                        after_impact: json!({"status":"not_requested"}),
                    });
                }
            }
            complete &= oc && nc;
            rows.push(Row { file: file.clone(), before_file: Some(old_path.clone()), after_file: Some(file), change: "renamed",
                classification: if old.is_some_and(|b| b.binary) { "binary" } else if source_wanted(old_path) { "source" } else { "unsupported_file_type" },
                before_mode: Some(a.mode.clone()), after_mode: Some(b.mode.clone()), before_hash: old.map(|b| b.hash.clone()), after_hash: new.map(|b| b.hash.clone()),
                hunks: vec![], symbols, file_level: true, complete: oc && nc,
                warnings: vec!["File move paired by unique identical content; symbol version association is heuristic".into()] });
            continue;
        }
        let (a, b) = (before.get(&file), after.get(&file));
        if same(a, b) {
            continue;
        }
        if opts.no_tests && crate::query::is_test_file(&file) {
            filtered_tests += 1;
            continue;
        }
        let (old, new) = (
            a.and_then(|f| f.blob.as_ref()),
            b.and_then(|f| f.blob.as_ref()),
        );
        let classification = if a.into_iter().chain(b).any(|f| f.mode == "160000") {
            "submodule"
        } else if a.into_iter().chain(b).any(|f| f.mode == "120000") {
            "symlink"
        } else if a.is_some()
            && b.is_some()
            && old.map(|b| &b.hash) == new.map(|b| &b.hash)
            && a.map(|f| &f.mode) != b.map(|f| &f.mode)
        {
            "mode_only"
        } else if old.into_iter().chain(new).any(|b| b.binary) {
            "binary"
        } else if !source_wanted(&file) {
            "unsupported_file_type"
        } else if old.into_iter().chain(new).any(|b| b.bytes > MAX_SOURCE) {
            "oversized_source"
        } else {
            "source"
        };
        let (hs, hcomplete) = if classification == "source" {
            hunks(
                old.and_then(|b| b.source.as_ref())
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or(""),
                new.and_then(|b| b.source.as_ref())
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or(""),
            )
        } else {
            (vec![], true)
        };
        let (symbols, scomplete, file_level) = if classification == "source" {
            entity_changes(&file, old, new, &hs)
        } else {
            (vec![], true, true)
        };
        complete &= hcomplete && scomplete;
        rows.push(Row {
            before_file: a.map(|_| file.clone()),
            after_file: b.map(|_| file.clone()),
            file,
            change: if a.is_none() {
                "added"
            } else if b.is_none() {
                "deleted"
            } else {
                "modified"
            },
            classification,
            before_mode: a.map(|f| f.mode.clone()),
            after_mode: b.map(|f| f.mode.clone()),
            before_hash: old.map(|b| b.hash.clone()),
            after_hash: new.map(|b| b.hash.clone()),
            hunks: hs,
            symbols,
            file_level,
            complete: hcomplete && scomplete,
            warnings: if !hcomplete {
                vec!["line diff work limit exceeded; coarse hunk disclosed".into()]
            } else if !scomplete {
                vec!["symbol parsing incomplete".into()]
            } else {
                vec![]
            },
        });
    }
    if opts.impact {
        let old_sources = sources(&before);
        let new_sources = sources(&after);
        let old_index = Index::from_sources(&root, &old_sources.files);
        let new_index = Index::from_sources(&root, &new_sources.files);
        let mut old_graph = crate::impact::Graph::new(&old_index, &old_sources);
        let mut new_graph = crate::impact::Graph::new(&new_index, &new_sources);
        for row in &mut rows {
            for symbol in &mut row.symbols {
                symbol.before_impact =
                    attach_impact(&mut old_graph, symbol.before.as_ref(), opts.max_depth);
                symbol.after_impact =
                    attach_impact(&mut new_graph, symbol.after.as_ref(), opts.max_depth);
            }
        }
    }
    let untracked = if head.is_none() {
        git(&repo, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .count()
    } else {
        0
    };
    let outside_files = after_tree
        .keys()
        .filter(|p| p.strip_prefix(prefix).is_err())
        .count();
    let uninspected_submodules = if head.is_none() && !opts.staged {
        after.values().filter(|f| f.mode == "160000").count()
    } else {
        0
    };
    complete &= uninspected_submodules == 0;
    let outside_changes = if opts.staged || head.is_some() {
        Some(
            before_tree
                .keys()
                .chain(after_tree.keys())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|p| p.strip_prefix(prefix).is_err())
                .filter(|p| {
                    before_tree.get(*p).map(|e| (&e.mode, &e.oid))
                        != after_tree.get(*p).map(|e| (&e.mode, &e.oid))
                })
                .count(),
        )
    } else {
        None
    };
    let mut freshness = crate::index::Freshness::empty(crate::index::FreshnessMode::Verified);
    freshness.mode = if head.is_some() || opts.staged {
        "git"
    } else {
        "captured_worktree"
    };
    freshness.files_checked = after.len();
    Ok((
        Report {
            analysis: json!({"snapshot":snapshot,"comparison":if opts.staged{"staged"}else if head.is_some(){if opts.merge_base{"merge_base"}else{"commits"}}else{"worktree"},"base_oid":base,"head_oid":head,"untracked_excluded":untracked,"outside_scope_tracked_files":outside_files,"outside_scope_changes":outside_changes,"uninspected_submodules":uninspected_submodules,"filtered_tests":filtered_tests,"impact_requested":opts.impact,"limitations":"Raw tracked bytes, no repository filters; working files captured sequentially, not an atomic worktree. Symbol matching is source-based/heuristic, not semantic change proof. Only unique identical-content file moves are paired. Submodule working-directory dirtiness and non-source hunks are not analyzed."}),
            rows,
            complete,
            warnings: vec![
                "Untracked files excluded; file-level and non-source changes are retained".into(),
            ],
            error: None,
        },
        freshness,
    ))
}

fn compact_impact(value: Value) -> Value {
    let Some(rows) = value.get("results").and_then(Value::as_array) else {
        return value;
    };
    let compact: Vec<_> = rows
        .iter()
        .map(|row| {
            let evidence = if row["supported"].is_object() {
                "supported"
            } else if row["possible"].is_object() {
                "possible"
            } else {
                "none"
            };
            json!({"symbol":row["symbol"],"depth":row["depth"],"evidence":evidence})
        })
        .collect();
    json!({"status":value["status"],"results":compact,"analysis":{
        "complete":value["analysis"]["complete"],"snapshot":value["analysis"]["snapshot"],
        "stop_reasons":value["analysis"]["stop_reasons"],"frontier_count":value["analysis"]["frontier_count"],
        "supported_count":value["analysis"]["supported_count"],"possible_only_count":value["analysis"]["possible_only_count"]},
        "error":value["error"]})
}
fn compact_entity(entity: Option<Entity>) -> Value {
    entity.map_or(Value::Null, |e| {
        json!({"name":e.name,"qualified":e.qualified,"kind":e.kind,"role":e.role,
        "line":e.line,"byte_range":e.byte_range})
    })
}
pub fn compact(report: Report<Row>) -> Report<Value> {
    let rows = report.rows.into_iter().map(|row| {
        let symbols = row.symbols.into_iter().map(|symbol| {
            let basis = if symbol.match_basis.contains("heuristic") { "heuristic" }
                else if symbol.match_basis == "unmatched_old_site" { "unmatched_old" }
                else if symbol.match_basis == "unmatched_new_site" { "unmatched_new" }
                else { symbol.match_basis };
            let mut value=json!({"change":symbol.change,"before":compact_entity(symbol.before),
                "after":compact_entity(symbol.after),"match":basis});
            if symbol.before_impact["status"] != "not_requested" { value["before_impact"] = compact_impact(symbol.before_impact); }
            if symbol.after_impact["status"] != "not_requested" { value["after_impact"] = compact_impact(symbol.after_impact); }
            value
        }).collect::<Vec<_>>();
        let mut value=json!({"file":row.file,"change":row.change,"classification":row.classification,
            "symbols":symbols,"file_level":row.file_level});
        if row.before_file.as_ref().is_some_and(|p|p != &row.file) { value["before_file"] = json!(row.before_file); }
        if row.after_file.as_ref().is_some_and(|p|p != &row.file) { value["after_file"] = json!(row.after_file); }
        if !row.complete { value["complete"] = json!(false); }
        if !row.warnings.is_empty() { value["warnings"] = json!(row.warnings); }
        value
    }).collect();
    Report {
        rows,
        analysis: {
            let mut a = report.analysis;
            a["detail"] = json!("compact");
            a["detail_omitted"] = json!([
                "raw_hunks",
                "hashes",
                "modes",
                "signatures",
                "full_impact_paths"
            ]);
            a["limitations"] = json!([
                "raw_tracked_bytes",
                "heuristic_version_match",
                "non_atomic_worktree",
                "non_source_hunks_unanalyzed"
            ]);
            a
        },
        complete: report.complete,
        warnings: report.warnings,
        error: report.error,
    }
}
