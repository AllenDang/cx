use ignore::WalkBuilder;
use rayon::prelude::*;
use redb::{Database, ReadOnlyDatabase, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::language::{
    LangError, detect_language, download_names_for, parse_and_extract, primary_extension,
};

pub const INDEX_VERSION: u32 = 12;

/// Compute the cache path for a given project root.
/// Returns `~/.cache/cx/indexes/<hash>.db` where hash is derived from the
/// canonical identity of the root (roadmap §4.1), so aliased spellings of the
/// same directory share one index.
pub fn cache_path_for(root: &Path) -> PathBuf {
    let canonical = crate::util::path::canonical(root);
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    let hash = hasher.finish();
    let dir = index_cache_dir();
    dir.join(format!("{hash:016x}.db"))
}

fn index_cache_dir() -> PathBuf {
    crate::lang::cx_cache_dir().join("indexes")
}

const META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const FILES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("files");
const SYMBOLS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("symbols");
const IMPORTS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("imports");

pub struct Index {
    /// Canonical project root: absolute, normalized, symlinks resolved.
    /// Every indexed path is stored relative to this root.
    pub root: PathBuf,
    db: Option<Database>,
    /// In-memory mirror for fast query access.
    pub entries: HashMap<PathBuf, FileData>,
    /// What this process did to establish that the index matches disk.
    pub freshness: Freshness,
    /// Paths re-parsed by this process, in scan order.  Evidence for
    /// `cx refresh`: these files are provably part of `freshness.generation`.
    pub updated: Vec<PathBuf>,
    /// Paths dropped from the index by this process because they are gone.
    pub removed: Vec<PathBuf>,
}

/// How cx decided whether the index still matches the working tree
/// (roadmap §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum FreshnessMode {
    /// Compare size + high-resolution mtime. Fast default, no file reads.
    Metadata,
    /// Hash file contents. Catches edits that preserve size and mtime.
    Verified,
    /// Only the paths the caller named were checked, by content hash.
    /// Selected by `cx refresh <paths>`, not by `--fresh`.
    #[value(skip)]
    Paths,
}

impl FreshnessMode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Verified => "verified",
            Self::Paths => "paths",
        }
    }
}

/// What the caller asked cx to verify before answering.
pub struct FreshnessRequest {
    pub mode: FreshnessMode,
    /// Paths to check in `Paths` mode; ignored otherwise.
    pub paths: Vec<PathBuf>,
}

impl FreshnessRequest {
    pub const fn new(mode: FreshnessMode, paths: Vec<PathBuf>) -> Self {
        Self { mode, paths }
    }
}

/// Observable freshness facts for one command (roadmap §4.4, §6.1).
///
/// Reported so an agent can prove which generation answered its query and how
/// that generation was established, instead of trusting that cx "probably"
/// noticed the edit.
#[derive(Debug, Clone, Serialize)]
pub struct Freshness {
    /// Monotonic index generation, bumped on every committed write.
    pub generation: u64,
    pub mode: &'static str,
    /// Files compared against the index by this command.
    pub files_checked: usize,
    /// Files re-parsed because they had changed.
    pub files_updated: usize,
    /// Files removed from the index because they are gone from disk.
    pub files_removed: usize,
    /// Indexable files skipped because their grammar is not installed.
    pub files_skipped_missing_grammar: usize,
}

impl Freshness {
    pub const fn empty(mode: FreshnessMode) -> Self {
        Self {
            generation: 0,
            mode: mode.as_str(),
            files_checked: 0,
            files_updated: 0,
            files_removed: 0,
            files_skipped_missing_grammar: 0,
        }
    }
}

enum CrawlResult {
    Indexed(PathBuf, FileData),
    MissingLang(String),
    ReadFailed(PathBuf, std::io::Error),
    ParseFailed,
}

#[derive(Debug, Clone)]
pub struct FileData {
    pub meta: FileEntry,
    pub symbols: Vec<Symbol>,
    /// Import/include targets as written in the source (roadmap §8).
    /// Empty for languages whose imports cx does not model.
    pub imports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub mtime_secs: u64,
    pub mtime_nanos: u32,
    /// File length in bytes, so a same-mtime edit that changes length is caught
    /// by the cheap metadata check.
    pub size: u64,
    /// Hash of the file contents at index time.  Free to record (the file was
    /// read to parse it) and the only way `verified` mode can detect an edit
    /// that preserved both size and mtime.
    pub content_hash: u64,
    pub language: String,
}

impl FileEntry {
    fn new(mtime: SystemTime, size: u64, content_hash: u64, language: &str) -> Self {
        let dur = mtime.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
        Self {
            mtime_secs: dur.as_secs(),
            mtime_nanos: dur.subsec_nanos(),
            size,
            content_hash,
            language: language.to_string(),
        }
    }

    pub fn mtime(&self) -> SystemTime {
        UNIX_EPOCH + Duration::new(self.mtime_secs, self.mtime_nanos)
    }
}

/// Hash file contents for freshness comparison.
///
/// Not a cryptographic digest: this only has to detect accidental edits, and it
/// runs over every indexable file in `verified` mode.
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Whether this location defines the symbol or only declares it.
    /// Independent of `kind` (roadmap §5.1): a C++ function prototype and its
    /// definition share `kind = Fn` but differ in role.
    #[serde(default)]
    pub role: SymbolRole,
    /// Lexical containers enclosing this symbol, outermost first
    /// (e.g. `["ange", "EcsWorld"]` for `ange::EcsWorld::run`).
    ///
    /// Empty means either genuinely top-level or not modelled — check
    /// `qualified_name` to tell those apart.
    #[serde(default)]
    pub scope_path: Vec<String>,
    /// Fully qualified lexical name, or `None` when cx does not model this
    /// language's scopes yet (roadmap §5.3).
    ///
    /// `None` is deliberate: reporting the bare name as "qualified" would claim
    /// a resolution that never happened.
    #[serde(default)]
    pub qualified_name: Option<String>,
    pub signature: String,
    pub byte_range: (usize, usize),
    /// Whether this symbol is a test (e.g. `#[test]` in Rust, `test` block in Zig).
    #[serde(default)]
    pub is_test: bool,
}

/// Serializable identity for a symbol, separate from its display name
/// (roadmap §5.2).
///
/// Derived rather than stored: the index keeps the facts (language, qualified
/// name, kind, signature) and this composes them on demand, so an index rewrite
/// is not needed to change the identity scheme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StableSymbolId {
    pub language: String,
    /// Qualified name when known, otherwise the bare name.
    pub name: String,
    /// False when `name` is unqualified because scopes are not modelled.
    pub qualified: bool,
    pub kind: SymbolKind,
    /// Distinguishes overloads that share a qualified name.
    pub signature_key: Option<String>,
}

impl StableSymbolId {
    /// Identity ignoring overload signature.
    ///
    /// A declaration and its definition share this key, which lets a query tell
    /// "one symbol seen at two locations" apart from "several distinct symbols
    /// that happen to share a short name" (roadmap §6.2).
    pub fn logical_key(&self) -> (&str, &str, SymbolKind) {
        (&self.language, &self.name, self.kind)
    }
}

impl Symbol {
    /// Identity for this symbol in the given language.
    ///
    /// A declaration and its definition intentionally produce the same id: they
    /// are one logical symbol observed at two locations, and `role` plus the
    /// location keep them distinguishable.
    pub fn stable_id(&self, language: &str) -> StableSymbolId {
        StableSymbolId {
            language: language.to_string(),
            name: self
                .qualified_name
                .clone()
                .unwrap_or_else(|| self.name.clone()),
            qualified: self.qualified_name.is_some(),
            kind: self.kind,
            signature_key: (!self.signature.is_empty()).then(|| self.signature.clone()),
        }
    }
}

/// What a symbol location *is*, as opposed to what kind of thing it names.
///
/// Only ever set from an explicit grammar capture — never guessed from the
/// presence of `{}` (roadmap §5.1).  A language whose query cannot tell the
/// forms apart yields [`SymbolRole::Unknown`] rather than a hopeful
/// `Definition`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum SymbolRole {
    /// The implementation: a body, a type with members, a namespace block.
    #[default]
    Definition,
    /// A signature-only site: C/C++ prototype, in-class method declaration,
    /// forward type declaration, Rust trait method signature, TypeScript
    /// interface/abstract member, `declare` ambient statement.
    Declaration,
    /// A document section (Markdown heading) — neither of the above.
    Heading,
    /// The grammar cannot reliably distinguish the forms for this construct.
    Unknown,
}

impl SymbolRole {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Declaration => "declaration",
            Self::Heading => "heading",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum SymbolKind {
    Fn,
    Struct,
    Enum,
    Trait,
    Type,
    Const,
    Class,
    Interface,
    Module,
    Event,
    Field,
    Heading,
}

impl SymbolKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Fn => "fn",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Type => "type",
            Self::Const => "const",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Module => "module",
            Self::Event => "event",
            Self::Field => "field",
            Self::Heading => "heading",
        }
    }
}

fn encode_file_entry(entry: &FileEntry) -> Vec<u8> {
    bincode::serialize(entry).expect("FileEntry serialization should not fail")
}

fn decode_file_entry(bytes: &[u8]) -> Option<FileEntry> {
    bincode::deserialize(bytes).ok()
}

/// Open the database exclusively, retrying on lock contention.
fn open_db_exclusive(path: &Path) -> Result<Database, redb::DatabaseError> {
    let mut attempts = 0;
    loop {
        match Database::create(path) {
            Ok(db) => return Ok(db),
            Err(redb::DatabaseError::DatabaseAlreadyOpen) if attempts < 20 => {
                attempts += 1;
                if attempts == 1 {
                    eprintln!("cx: database locked, waiting...");
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Load entries and the generation counter from a readable database.
fn load_entries(db: &impl ReadableDatabase) -> Option<(HashMap<PathBuf, FileData>, u64)> {
    let read_txn = db.begin_read().ok()?;
    let generation = read_generation(&read_txn);

    // Check version
    let version_ok = (|| -> Option<bool> {
        let table = read_txn.open_table(META_TABLE).ok()?;
        let val = table.get("version").ok()??;
        let bytes = val.value();
        if bytes.len() == 4 {
            Some(u32::from_le_bytes(bytes.try_into().unwrap()) == INDEX_VERSION)
        } else {
            None
        }
    })()
    .unwrap_or(false);

    if !version_ok {
        return None;
    }

    let mut entries: HashMap<PathBuf, FileData> = HashMap::new();

    if let Ok(table) = read_txn.open_table(FILES_TABLE) {
        for item in table.iter().into_iter().flatten() {
            let Ok((key, val)) = item else { continue };
            let path = PathBuf::from(key.value());
            if let Some(meta) = decode_file_entry(val.value()) {
                entries.insert(
                    path,
                    FileData {
                        meta,
                        symbols: Vec::new(),
                        imports: Vec::new(),
                    },
                );
            }
        }
    }
    if let Ok(table) = read_txn.open_table(SYMBOLS_TABLE) {
        for item in table.iter().into_iter().flatten() {
            let Ok((key, val)) = item else { continue };
            let path = PathBuf::from(key.value());
            let syms: Vec<Symbol> = bincode::deserialize(val.value()).unwrap_or_default();
            if let Some(data) = entries.get_mut(&path) {
                data.symbols = syms;
            }
        }
    }
    if let Ok(table) = read_txn.open_table(IMPORTS_TABLE) {
        for item in table.iter().into_iter().flatten() {
            let Ok((key, val)) = item else { continue };
            let path = PathBuf::from(key.value());
            let imports: Vec<String> = bincode::deserialize(val.value()).unwrap_or_default();
            if let Some(data) = entries.get_mut(&path) {
                data.imports = imports;
            }
        }
    }

    Some((entries, generation))
}

/// Read the persisted generation counter (0 when absent).
fn read_generation(read_txn: &redb::ReadTransaction) -> u64 {
    (|| -> Option<u64> {
        let table = read_txn.open_table(META_TABLE).ok()?;
        let val = table.get("generation").ok()??;
        let bytes = val.value();
        Some(u64::from_le_bytes(bytes.try_into().ok()?))
    })()
    .unwrap_or(0)
}

/// One indexable file found on disk, with the metadata needed to compare it
/// against the index.
struct DiskFile {
    rel_path: PathBuf,
    mtime: SystemTime,
    lang: &'static str,
}

/// The difference between the index and the working tree.
struct DiskScan {
    /// Files that must be (re)parsed.
    stale: Vec<DiskFile>,
    /// Indexed paths that no longer exist on disk.
    deleted: Vec<PathBuf>,
    /// How many files this scan actually compared.
    files_checked: usize,
    /// Indexable files skipped because their grammar is missing.
    skipped_missing_grammar: usize,
}

impl DiskScan {
    fn is_clean(&self) -> bool {
        self.stale.is_empty() && self.deleted.is_empty()
    }
}

/// Compare the working tree against the index according to `req`.
///
/// `metadata` compares size + high-resolution mtime and reads no file bodies.
/// `verified` hashes contents, so it catches an edit that preserved both size
/// and mtime.  `paths` checks only the paths the caller named, by content hash.
fn scan_disk(
    root: &Path,
    entries: &HashMap<PathBuf, FileData>,
    req: &FreshnessRequest,
) -> DiskScan {
    if req.mode == FreshnessMode::Paths {
        return scan_named_paths(root, entries, &req.paths);
    }

    // Languages known to be usable: either already represented in the index or
    // with every required grammar installed.
    let indexed_langs: HashSet<&str> = entries.values().map(|d| d.meta.language.as_str()).collect();
    let installed_grammars = tree_sitter_language_pack::downloaded_languages();

    let mut scan = DiskScan {
        stale: Vec::new(),
        deleted: Vec::new(),
        files_checked: 0,
        skipped_missing_grammar: 0,
    };
    let mut seen: HashSet<PathBuf> = HashSet::with_capacity(entries.len());

    for entry in walk(root) {
        let path = entry.path();
        let Some(lang) = detect_language(path) else {
            continue;
        };
        let Ok(rel_path) = path.strip_prefix(root) else {
            continue;
        };
        let rel_path = rel_path.to_path_buf();

        let metadata = entry.metadata().ok();
        let mtime = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let size = metadata.as_ref().map_or(0, std::fs::Metadata::len);

        match entries.get(&rel_path) {
            Some(data) => {
                seen.insert(rel_path.clone());
                scan.files_checked += 1;
                let changed = match req.mode {
                    FreshnessMode::Verified => {
                        // Content is the authority; mtime is not consulted.
                        fs::read(path)
                            .is_ok_and(|bytes| content_hash(&bytes) != data.meta.content_hash)
                    }
                    FreshnessMode::Metadata | FreshnessMode::Paths => {
                        data.meta.mtime() != mtime || data.meta.size != size
                    }
                };
                if changed {
                    scan.stale.push(DiskFile {
                        rel_path,
                        mtime,
                        lang,
                    });
                }
            }
            None => {
                let grammar_installed = download_names_for(lang)
                    .iter()
                    .all(|name| installed_grammars.iter().any(|installed| installed == name));
                if indexed_langs.contains(lang) || grammar_installed {
                    scan.files_checked += 1;
                    scan.stale.push(DiskFile {
                        rel_path,
                        mtime,
                        lang,
                    });
                } else {
                    scan.skipped_missing_grammar += 1;
                }
            }
        }
    }

    // Anything indexed but not seen on disk is gone.
    for path in entries.keys() {
        if !seen.contains(path) {
            scan.deleted.push(path.clone());
        }
    }

    scan
}

/// `paths` mode: check exactly the paths the caller named, by content hash.
///
/// This is the mechanical guarantee an agent needs after editing files: name
/// them, and they are in the next generation regardless of clock granularity.
fn scan_named_paths(
    root: &Path,
    entries: &HashMap<PathBuf, FileData>,
    paths: &[PathBuf],
) -> DiskScan {
    let mut scan = DiskScan {
        stale: Vec::new(),
        deleted: Vec::new(),
        files_checked: 0,
        skipped_missing_grammar: 0,
    };

    for requested in paths {
        let abs = crate::util::path::canonical(requested);
        let Ok(rel_path) = abs.strip_prefix(root) else {
            eprintln!(
                "cx: {} is outside the project root, skipping",
                requested.display()
            );
            continue;
        };
        let rel_path = rel_path.to_path_buf();
        scan.files_checked += 1;

        if !abs.exists() {
            if entries.contains_key(&rel_path) {
                scan.deleted.push(rel_path);
            }
            continue;
        }

        let Some(lang) = detect_language(&abs) else {
            eprintln!("cx: {} has no known grammar, skipping", requested.display());
            scan.skipped_missing_grammar += 1;
            continue;
        };

        let metadata = fs::metadata(&abs).ok();
        let mtime = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        // Content hash is the authority here: an agent that names a path after
        // editing it must get a re-parse even if size and mtime look unchanged.
        let unchanged = entries.get(&rel_path).is_some_and(|data| {
            fs::read(&abs).is_ok_and(|bytes| content_hash(&bytes) == data.meta.content_hash)
        });
        if !unchanged {
            scan.stale.push(DiskFile {
                rel_path,
                mtime,
                lang,
            });
        }
    }

    scan
}

impl Index {
    /// Load or build the index for the given project root.
    ///
    /// `root` is canonicalized first so the cache key, `Index.root`, and the
    /// relative paths derived from it all share one identity.
    ///
    /// Tries a shared (read-only) open first so multiple cx processes can
    /// run concurrently.  Falls back to an exclusive open only when the
    /// index needs to be created or updated.
    pub fn load_or_build(root: &Path, req: &FreshnessRequest) -> Self {
        let root = crate::util::path::canonical(root);
        let root = root.as_path();
        let db_path = cache_path_for(root);
        if let Some(parent) = db_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Fast path: open read-only (shared lock) and check if index is fresh
        if db_path.exists() {
            match ReadOnlyDatabase::open(&db_path) {
                Ok(ro_db) => {
                    if let Some((entries, generation)) = load_entries(&ro_db) {
                        let scan = scan_disk(root, &entries, req);
                        if scan.is_clean() {
                            let freshness = Freshness {
                                generation,
                                mode: req.mode.as_str(),
                                files_checked: scan.files_checked,
                                files_updated: 0,
                                files_removed: 0,
                                files_skipped_missing_grammar: scan.skipped_missing_grammar,
                            };
                            return Self {
                                root: root.to_path_buf(),
                                db: None,
                                entries,
                                freshness,
                                updated: Vec::new(),
                                removed: Vec::new(),
                            };
                        }
                        // Stale: fall through to the exclusive path, which
                        // rescans rather than trusting this scan, since another
                        // process may write in between.
                    }
                }
                Err(redb::DatabaseError::UpgradeRequired(_)) => {
                    // Old redb format; delete so exclusive path recreates it
                    let _ = fs::remove_file(&db_path);
                }
                Err(_) => {}
            }
        }

        // Slow path: need exclusive access to create or update the index
        let db = match open_db_exclusive(&db_path) {
            Ok(db) => db,
            Err(redb::DatabaseError::UpgradeRequired(_)) => {
                // Old redb format (e.g. v2 → v3 upgrade); delete and recreate
                let _ = fs::remove_file(&db_path);
                match open_db_exclusive(&db_path) {
                    Ok(db) => db,
                    Err(e) => {
                        eprintln!("cx: failed to open database: {e}");
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("cx: failed to open database: {e}");
                std::process::exit(1);
            }
        };

        if let Some((entries, generation)) = load_entries(&db) {
            let mut idx = Self {
                root: root.to_path_buf(),
                db: Some(db),
                entries,
                freshness: Freshness {
                    generation,
                    ..Freshness::empty(req.mode)
                },
                updated: Vec::new(),
                removed: Vec::new(),
            };
            let scan = scan_disk(root, &idx.entries, req);
            idx.apply_scan(scan);
            idx
        } else {
            let mut idx = Self {
                root: root.to_path_buf(),
                db: Some(db),
                entries: HashMap::new(),
                freshness: Freshness::empty(req.mode),
                updated: Vec::new(),
                removed: Vec::new(),
            };
            idx.full_crawl();
            idx.save_all();
            idx
        }
    }

    /// Crawl from project root, collecting all supported files.
    fn full_crawl(&mut self) {
        let mut missing_langs: HashMap<String, usize> = HashMap::new();

        // Collect files first so we can show progress
        let files: Vec<_> = walk(&self.root)
            .filter_map(|entry| {
                let path = entry.path();
                let lang = detect_language(path)?;
                let rel_path = path.strip_prefix(&self.root).ok()?.to_path_buf();
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                Some((path.to_path_buf(), rel_path, lang, mtime))
            })
            .collect();

        let total = files.len();
        if total > 0 {
            eprintln!("cx: indexing {total} files...");
        }

        let completed = AtomicUsize::new(0);
        let progress_step = if total >= 100 { Some(total / 10) } else { None };
        let results: Vec<_> = files
            .par_iter()
            .map(|(abs_path, rel_path, lang, mtime)| {
                let result = match fs::read(abs_path) {
                    Ok(source) => match parse_and_extract(lang, &source, abs_path) {
                        Ok(parse) => CrawlResult::Indexed(
                            rel_path.clone(),
                            FileData {
                                // Size and hash come from the bytes just read,
                                // so recording them costs no extra I/O.
                                meta: FileEntry::new(
                                    *mtime,
                                    source.len() as u64,
                                    content_hash(&source),
                                    lang,
                                ),
                                symbols: parse.symbols,
                                imports: parse.imports,
                            },
                        ),
                        Err(LangError::NotInstalled(name)) => CrawlResult::MissingLang(name),
                        Err(_) => CrawlResult::ParseFailed,
                    },
                    Err(e) => CrawlResult::ReadFailed(abs_path.clone(), e),
                };

                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(step) = progress_step
                    && done.is_multiple_of(step)
                {
                    eprintln!("cx: indexed {done}/{total}...");
                }

                result
            })
            .collect();

        for result in results {
            match result {
                CrawlResult::Indexed(rel_path, data) => {
                    self.entries.insert(rel_path, data);
                }
                CrawlResult::MissingLang(name) => {
                    *missing_langs.entry(name).or_insert(0) += 1;
                }
                CrawlResult::ReadFailed(path, e) => {
                    eprintln!("cx: warning: failed to read {}: {}", path.display(), e);
                }
                CrawlResult::ParseFailed => {}
            }
        }

        // A full crawl checked and indexed everything it could.
        self.freshness.files_checked = total;
        self.freshness.files_updated = self.entries.len();
        self.freshness.files_skipped_missing_grammar = missing_langs.values().sum();

        // UX: warn about missing grammars
        if !missing_langs.is_empty() {
            if self.entries.is_empty() {
                // No files indexed at all
                eprintln!("cx: no language grammars installed\n");
                eprintln!("Detected languages in this project:");
                let mut langs: Vec<_> = missing_langs.iter().collect();
                langs.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
                for (lang, count) in &langs {
                    eprintln!("  {lang} ({count} files)");
                }
                let names: Vec<&str> = langs.iter().map(|(n, _)| n.as_str()).collect();
                eprintln!("\nInstall with: cx lang add {}", names.join(" "));
            } else {
                // Some files indexed, some missing
                for lang in missing_langs.keys() {
                    let ext = primary_extension(lang);
                    eprintln!("cx: skipping .{ext} files — install with: cx lang add {lang}");
                }
            }
        }
    }

    /// Apply a completed scan: re-parse stale files, drop deleted ones, persist
    /// the result, and bump the index generation.
    ///
    /// Freshness counters are recorded here so a query can report exactly how
    /// many files this process checked and updated (roadmap §4.4).
    fn apply_scan(&mut self, scan: DiskScan) {
        let mut missing_langs: HashSet<String> = HashSet::new();

        for path in &scan.deleted {
            self.entries.remove(path);
        }

        let total = scan.stale.len();
        if total > 0 {
            eprintln!("cx: updating {total} files...");
        }

        let mut changed_paths: Vec<PathBuf> = Vec::new();
        for (i, file) in scan.stale.iter().enumerate() {
            if total >= 100 && (i + 1) % (total / 10) == 0 {
                eprintln!("cx: indexed {}/{}...", i + 1, total);
            }
            let abs_path = self.root.join(&file.rel_path);
            let Ok(source) = fs::read(&abs_path) else {
                continue;
            };
            let parse = match parse_and_extract(file.lang, &source, &abs_path) {
                Ok(parse) => parse,
                Err(LangError::NotInstalled(name)) => {
                    missing_langs.insert(name);
                    continue;
                }
                Err(_) => continue,
            };
            self.entries.insert(
                file.rel_path.clone(),
                FileData {
                    // Trust the bytes just read over the stat() results, so the
                    // stored size and hash always describe the parsed content.
                    meta: FileEntry::new(
                        file.mtime,
                        source.len() as u64,
                        content_hash(&source),
                        file.lang,
                    ),
                    symbols: parse.symbols,
                    imports: parse.imports,
                },
            );
            changed_paths.push(file.rel_path.clone());
        }

        for lang in &missing_langs {
            let ext = primary_extension(lang);
            eprintln!("cx: skipping .{ext} files — install with: cx lang add {lang}");
        }

        self.freshness.files_checked = scan.files_checked;
        self.freshness.files_updated = changed_paths.len();
        self.freshness.files_removed = scan.deleted.len();
        self.freshness.files_skipped_missing_grammar =
            scan.skipped_missing_grammar + missing_langs.len();
        self.updated = changed_paths.clone();
        self.removed = scan.deleted.clone();

        if scan.deleted.is_empty() && changed_paths.is_empty() {
            return;
        }

        let next_generation = self.freshness.generation + 1;
        let Some(ref db) = self.db else { return };
        let write_txn = match db.begin_write() {
            Ok(txn) => txn,
            Err(e) => {
                eprintln!("cx: failed to begin write for incremental update: {e}");
                return;
            }
        };
        {
            let Ok(mut files_table) = write_txn.open_table(FILES_TABLE) else {
                eprintln!("cx: failed to open files table — rebuild with: cx cache clean");
                return;
            };
            let Ok(mut syms_table) = write_txn.open_table(SYMBOLS_TABLE) else {
                eprintln!("cx: failed to open symbols table — rebuild with: cx cache clean");
                return;
            };
            let Ok(mut imports_table) = write_txn.open_table(IMPORTS_TABLE) else {
                eprintln!("cx: failed to open imports table — rebuild with: cx cache clean");
                return;
            };
            for path in &scan.deleted {
                let key = path.to_string_lossy();
                let _ = files_table.remove(key.as_ref());
                let _ = syms_table.remove(key.as_ref());
                let _ = imports_table.remove(key.as_ref());
            }
            for path in &changed_paths {
                if let Some(data) = self.entries.get(path) {
                    let key = path.to_string_lossy();
                    match bincode::serialize(&data.symbols) {
                        Ok(sym_bytes) => {
                            let entry_bytes = encode_file_entry(&data.meta);
                            let _ = files_table.insert(key.as_ref(), entry_bytes.as_slice());
                            let _ = syms_table.insert(key.as_ref(), sym_bytes.as_slice());
                        }
                        Err(e) => eprintln!("cx: failed to serialize symbols for {key}: {e}"),
                    }
                    if let Ok(import_bytes) = bincode::serialize(&data.imports) {
                        let _ = imports_table.insert(key.as_ref(), import_bytes.as_slice());
                    }
                }
            }
        }
        if let Ok(mut meta) = write_txn.open_table(META_TABLE) {
            let _ = meta.insert("generation", next_generation.to_le_bytes().as_slice());
        }
        if let Err(e) = write_txn.commit() {
            eprintln!("cx: failed to commit incremental update: {e}");
            return;
        }
        self.freshness.generation = next_generation;
    }

    /// Write the entire index to the database (used after `full_crawl`).
    /// Clears all existing data first to avoid stale entries.
    fn save_all(&mut self) {
        let next_generation = self.freshness.generation + 1;
        let Some(ref db) = self.db else { return };
        let write_txn = match db.begin_write() {
            Ok(txn) => txn,
            Err(e) => {
                eprintln!("cx: failed to begin write: {e}");
                return;
            }
        };

        // Delete and recreate tables to clear stale entries
        let _ = write_txn.delete_table(FILES_TABLE);
        let _ = write_txn.delete_table(SYMBOLS_TABLE);
        let _ = write_txn.delete_table(IMPORTS_TABLE);

        // Write version
        {
            let Ok(mut table) = write_txn.open_table(META_TABLE) else {
                eprintln!("cx: failed to open meta table — rebuild with: cx cache clean");
                return;
            };
            let _ = table.insert("version", INDEX_VERSION.to_le_bytes().as_slice());
            // A rebuild is still a new generation, so a reader can tell that the
            // index it saw before is not the one answering now.
            let _ = table.insert("generation", next_generation.to_le_bytes().as_slice());
        }

        // Write files, symbols and imports
        {
            let Ok(mut files_table) = write_txn.open_table(FILES_TABLE) else {
                eprintln!("cx: failed to open files table — rebuild with: cx cache clean");
                return;
            };
            let Ok(mut syms_table) = write_txn.open_table(SYMBOLS_TABLE) else {
                eprintln!("cx: failed to open symbols table — rebuild with: cx cache clean");
                return;
            };
            let Ok(mut imports_table) = write_txn.open_table(IMPORTS_TABLE) else {
                eprintln!("cx: failed to open imports table — rebuild with: cx cache clean");
                return;
            };
            for (path, data) in &self.entries {
                let key = path.to_string_lossy();
                let entry_bytes = encode_file_entry(&data.meta);
                let _ = files_table.insert(key.as_ref(), entry_bytes.as_slice());
                match bincode::serialize(&data.symbols) {
                    Ok(sym_bytes) => {
                        let _ = syms_table.insert(key.as_ref(), sym_bytes.as_slice());
                    }
                    Err(e) => eprintln!("cx: failed to serialize symbols for {key}: {e}"),
                }
                if let Ok(import_bytes) = bincode::serialize(&data.imports) {
                    let _ = imports_table.insert(key.as_ref(), import_bytes.as_slice());
                }
            }
        }

        if let Err(e) = write_txn.commit() {
            eprintln!("cx: failed to commit: {e}");
            return;
        }
        // Only claim the new generation once it is durably committed.
        self.freshness.generation = next_generation;
    }
}

/// Walk the project tree, respecting .gitignore and skipping the index/db files.
fn walk(root: &Path) -> impl Iterator<Item = ignore::DirEntry> {
    WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|e| {
            let name = e.file_name().to_str().unwrap_or("");
            if name == ".git" {
                return false;
            }
            if e.file_type().is_some_and(|ft| ft.is_dir()) && e.path().join(".cx-ignore").exists() {
                return false;
            }
            true
        })
        .build()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Once;

    static INIT: Once = Once::new();

    /// Default freshness request for tests: cheap metadata comparison.
    fn metadata_req() -> FreshnessRequest {
        FreshnessRequest::new(FreshnessMode::Metadata, Vec::new())
    }

    fn init_grammar_cache() {
        INIT.call_once(|| {
            let config = tree_sitter_language_pack::PackConfig {
                cache_dir: Some(crate::lang::grammar_cache_dir()),
                ..Default::default()
            };
            tree_sitter_language_pack::configure(&config)
                .expect("failed to configure grammar cache");
        });
    }

    #[test]
    fn test_file_entry_encode_roundtrip() {
        let entry = FileEntry::new(
            UNIX_EPOCH + Duration::new(1234567890, 42),
            4096,
            content_hash(b"fn main() {}"),
            "rust",
        );
        let bytes = encode_file_entry(&entry);
        let decoded = decode_file_entry(&bytes).expect("should decode");
        assert_eq!(entry.mtime(), decoded.mtime());
        assert_eq!(entry.language, decoded.language);
        assert_eq!(entry.size, decoded.size);
        assert_eq!(entry.content_hash, decoded.content_hash);
    }

    #[test]
    fn test_content_hash_detects_same_length_edits() {
        // The case metadata mode cannot see: same byte count, different bytes.
        assert_ne!(content_hash(b"fn a() {}"), content_hash(b"fn b() {}"));
        assert_eq!(content_hash(b"fn a() {}"), content_hash(b"fn a() {}"));
    }

    #[test]
    fn test_file_entry_decode_garbage_returns_none() {
        assert!(decode_file_entry(&[0u8; 5]).is_none());
        assert!(decode_file_entry(&[]).is_none());
    }

    #[test]
    fn test_symbol_bincode_roundtrip() {
        let symbols = vec![
            Symbol {
                name: "foo".to_string(),
                kind: SymbolKind::Fn,
                role: SymbolRole::Definition,
                scope_path: Vec::new(),
                qualified_name: Some("foo".to_string()),
                signature: "pub fn foo(x: i32) -> bool".to_string(),
                byte_range: (100, 500),
                is_test: false,
            },
            Symbol {
                name: "Bar".to_string(),
                kind: SymbolKind::Struct,
                role: SymbolRole::Definition,
                scope_path: vec!["outer".to_string()],
                qualified_name: Some("outer::Bar".to_string()),
                signature: "pub struct Bar".to_string(),
                byte_range: (600, 800),
                is_test: false,
            },
            Symbol {
                name: "test_bar".to_string(),
                kind: SymbolKind::Fn,
                role: SymbolRole::Declaration,
                scope_path: Vec::new(),
                qualified_name: None,
                signature: "fn test_bar()".to_string(),
                byte_range: (900, 1000),
                is_test: true,
            },
        ];
        let bytes = bincode::serialize(&symbols).unwrap();
        let decoded: Vec<Symbol> = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].name, "foo");
        assert!(!decoded[0].is_test);
        assert_eq!(decoded[1].kind, SymbolKind::Struct);
        assert_eq!(decoded[0].byte_range, (100, 500));
        assert_eq!(decoded[2].name, "test_bar");
        assert!(decoded[2].is_test);
        assert_eq!(decoded[0].role, SymbolRole::Definition);
        assert_eq!(decoded[2].role, SymbolRole::Declaration);
        assert_eq!(decoded[1].scope_path, vec!["outer".to_string()]);
        assert_eq!(decoded[1].qualified_name.as_deref(), Some("outer::Bar"));
        assert_eq!(
            decoded[2].qualified_name, None,
            "unmodelled scope stays unresolved rather than claiming the bare name"
        );
    }

    #[test]
    fn test_stable_id_separates_identity_from_display_name() {
        let qualified = Symbol {
            name: "run".to_string(),
            kind: SymbolKind::Fn,
            role: SymbolRole::Definition,
            scope_path: vec!["ange".to_string(), "EcsWorld".to_string()],
            qualified_name: Some("ange::EcsWorld::run".to_string()),
            signature: "void EcsWorld::run()".to_string(),
            byte_range: (0, 10),
            is_test: false,
        };
        let id = qualified.stable_id("cpp");
        assert_eq!(id.name, "ange::EcsWorld::run");
        assert!(id.qualified);

        // Same short name in another scope must not produce the same identity.
        let other = Symbol {
            scope_path: vec!["alpha".to_string()],
            qualified_name: Some("alpha::run".to_string()),
            signature: "void run()".to_string(),
            ..qualified.clone()
        };
        assert_ne!(id, other.stable_id("cpp"));

        // Unmodelled scope is reported as unqualified, not silently "qualified".
        let unmodelled = Symbol {
            scope_path: Vec::new(),
            qualified_name: None,
            ..qualified.clone()
        };
        let unmodelled_id = unmodelled.stable_id("lua");
        assert_eq!(unmodelled_id.name, "run");
        assert!(!unmodelled_id.qualified);
    }

    #[test]
    fn test_symbol_role_defaults_to_definition_for_legacy_rows() {
        // Rows written before the role field existed decode with the serde
        // default instead of failing, so a stale-but-compatible payload stays
        // readable; INDEX_VERSION still forces a rebuild for real migrations.
        assert_eq!(SymbolRole::default(), SymbolRole::Definition);
    }

    #[test]
    fn test_full_crawl_finds_rust_files() {
        init_grammar_cache();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test-crawl.db");
        let db = Database::create(&db_path).unwrap();
        // Use the real project root for crawling, but store db in tempdir
        let cwd = env::current_dir().unwrap();
        let mut idx = Index {
            root: cwd,
            db: Some(db),
            entries: HashMap::new(),
            freshness: Freshness::empty(FreshnessMode::Metadata),
            updated: Vec::new(),
            removed: Vec::new(),
        };
        idx.full_crawl();

        assert!(idx.entries.contains_key(&PathBuf::from("src/main.rs")));
        for path in idx.entries.keys() {
            assert!(!path.starts_with("target/"), "found target/ file: {path:?}");
        }
    }

    #[test]
    fn test_walk_respects_gitignore() {
        let cwd = env::current_dir().unwrap();
        let entries: Vec<_> = walk(&cwd).collect();
        for entry in &entries {
            let path = entry.path();
            let rel = path.strip_prefix(&cwd).unwrap_or(path);
            assert!(!rel.starts_with(".git/"), "found .git file: {rel:?}");
            assert!(!rel.starts_with("target/"), "found target file: {rel:?}");
        }
    }

    /// Helper: create a temp project with .git dir and source files, return (tempdir, Index).
    fn build_temp_index(files: &[(&str, &str)]) -> (tempfile::TempDir, Index) {
        init_grammar_cache();
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full, content).unwrap();
        }
        let idx = Index::load_or_build(dir.path(), &metadata_req());
        (dir, idx)
    }

    #[test]
    fn test_load_or_build_fresh_db() {
        let (dir, idx) = build_temp_index(&[
            ("src/main.rs", "fn main() {}\n"),
            ("src/lib.rs", "pub fn hello() {}\n"),
        ]);

        assert!(idx.entries.contains_key(&PathBuf::from("src/main.rs")));
        assert!(idx.entries.contains_key(&PathBuf::from("src/lib.rs")));
        assert_eq!(
            idx.entries
                .get(&PathBuf::from("src/main.rs"))
                .unwrap()
                .symbols
                .len(),
            1
        );
        assert_eq!(
            idx.entries
                .get(&PathBuf::from("src/lib.rs"))
                .unwrap()
                .symbols
                .len(),
            1
        );

        // DB file should exist in cache dir
        assert!(cache_path_for(dir.path()).exists());
    }

    #[test]
    fn test_full_crawl_indexes_many_files() {
        let files: Vec<_> = (0..64)
            .map(|i| {
                (
                    format!("src/module_{i}.rs"),
                    format!("pub fn function_{i}() -> usize {{ {i} }}\n"),
                )
            })
            .collect();
        let borrowed_files: Vec<_> = files
            .iter()
            .map(|(path, content)| (path.as_str(), content.as_str()))
            .collect();

        let (_dir, idx) = build_temp_index(&borrowed_files);

        assert_eq!(idx.entries.len(), 64);
        for i in 0..64 {
            let path = PathBuf::from(format!("src/module_{i}.rs"));
            let symbols = &idx.entries.get(&path).unwrap().symbols;
            assert!(
                symbols
                    .iter()
                    .any(|symbol| symbol.name == format!("function_{i}")),
                "missing function_{i} in {path:?}"
            );
        }
    }

    #[test]
    fn test_load_or_build_reloads_from_existing_db() {
        let (dir, idx) = build_temp_index(&[("src/main.rs", "fn main() {}\nfn helper() {}\n")]);

        let file_count = idx.entries.len();
        let sym_count = idx
            .entries
            .get(&PathBuf::from("src/main.rs"))
            .unwrap()
            .symbols
            .len();
        assert!(
            sym_count >= 2,
            "should have at least 2 symbols: {sym_count}"
        );

        // Drop and reload — should get same data from redb
        drop(idx);
        let idx2 = Index::load_or_build(dir.path(), &metadata_req());
        assert_eq!(idx2.entries.len(), file_count);
        assert_eq!(
            idx2.entries
                .get(&PathBuf::from("src/main.rs"))
                .unwrap()
                .symbols
                .len(),
            sym_count,
        );
    }

    #[test]
    fn test_save_all_clears_stale_entries() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.path().join("src/b.rs"), "fn b() {}\n").unwrap();

        // Build index with both files
        let idx = Index::load_or_build(dir.path(), &metadata_req());
        assert!(idx.entries.contains_key(&PathBuf::from("src/a.rs")));
        assert!(idx.entries.contains_key(&PathBuf::from("src/b.rs")));
        drop(idx);

        // Remove b.rs, rebuild
        fs::remove_file(dir.path().join("src/b.rs")).unwrap();
        let idx2 = Index::load_or_build(dir.path(), &metadata_req());
        assert!(idx2.entries.contains_key(&PathBuf::from("src/a.rs")));
        assert!(!idx2.entries.contains_key(&PathBuf::from("src/b.rs")));

        // Reload again — b.rs should still be gone from redb
        drop(idx2);
        let idx3 = Index::load_or_build(dir.path(), &metadata_req());
        assert!(!idx3.entries.contains_key(&PathBuf::from("src/b.rs")));
    }

    #[test]
    fn test_incremental_update_detects_new_file() {
        init_grammar_cache();
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "fn a() {}\n").unwrap();

        let idx = Index::load_or_build(dir.path(), &metadata_req());
        assert_eq!(idx.entries.len(), 1);
        drop(idx);

        // Set mtime in the future so the incremental update detects the new file
        // even on filesystems with coarse (1-second) timestamp granularity.
        let b_path = dir.path().join("src/b.rs");
        fs::write(&b_path, "fn b() {}\n").unwrap();
        let future = SystemTime::now() + Duration::from_secs(2);
        fs::File::options()
            .write(true)
            .open(&b_path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(future))
            .unwrap();

        let idx2 = Index::load_or_build(dir.path(), &metadata_req());
        assert_eq!(idx2.entries.len(), 2);
        assert!(idx2.entries.contains_key(&PathBuf::from("src/b.rs")));
        assert_eq!(
            idx2.entries
                .get(&PathBuf::from("src/b.rs"))
                .unwrap()
                .symbols
                .len(),
            1
        );
    }

    #[test]
    fn test_incremental_update_detects_modified_file() {
        init_grammar_cache();
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "fn a() {}\n").unwrap();

        let idx = Index::load_or_build(dir.path(), &metadata_req());
        assert_eq!(
            idx.entries
                .get(&PathBuf::from("src/a.rs"))
                .unwrap()
                .symbols
                .len(),
            1
        );
        drop(idx);

        // Modify the file — add a second function.
        // Set mtime in the future to avoid coarse-granularity timestamp ties.
        let a_path = dir.path().join("src/a.rs");
        fs::write(&a_path, "fn a() {}\nfn b() {}\n").unwrap();
        let future = SystemTime::now() + Duration::from_secs(2);
        fs::File::options()
            .write(true)
            .open(&a_path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(future))
            .unwrap();

        let idx2 = Index::load_or_build(dir.path(), &metadata_req());
        assert_eq!(
            idx2.entries
                .get(&PathBuf::from("src/a.rs"))
                .unwrap()
                .symbols
                .len(),
            2,
            "should detect modified file and re-parse symbols"
        );
    }

    #[test]
    fn test_incremental_update_detects_deleted_file() {
        init_grammar_cache();
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.path().join("src/b.rs"), "fn b() {}\n").unwrap();

        let idx = Index::load_or_build(dir.path(), &metadata_req());
        assert_eq!(idx.entries.len(), 2);
        drop(idx);

        // Delete one file
        fs::remove_file(dir.path().join("src/b.rs")).unwrap();

        let idx2 = Index::load_or_build(dir.path(), &metadata_req());
        assert_eq!(idx2.entries.len(), 1);
        assert!(idx2.entries.contains_key(&PathBuf::from("src/a.rs")));
        assert!(!idx2.entries.contains_key(&PathBuf::from("src/b.rs")));
    }

    #[test]
    fn test_version_mismatch_triggers_rebuild() {
        init_grammar_cache();
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "fn a() {}\n").unwrap();

        // Build normally
        let idx = Index::load_or_build(dir.path(), &metadata_req());
        assert!(idx.entries.contains_key(&PathBuf::from("src/a.rs")));
        drop(idx);

        // Corrupt the version in the db
        let db = Database::create(cache_path_for(dir.path())).unwrap();
        {
            let write_txn = db.begin_write().unwrap();
            {
                let mut table = write_txn.open_table(META_TABLE).unwrap();
                let _ = table.insert("version", 999u32.to_le_bytes().as_slice());
            }
            write_txn.commit().unwrap();
        }
        drop(db);

        // Reload — should detect version mismatch and rebuild
        let idx2 = Index::load_or_build(dir.path(), &metadata_req());
        assert!(idx2.entries.contains_key(&PathBuf::from("src/a.rs")));
    }

    #[test]
    fn test_pre_role_index_version_is_rebuilt_with_roles() {
        // Phase 2 bumped INDEX_VERSION 8 → 9 to add SymbolRole.  An index left
        // behind by an older cx must be rebuilt, not decoded with defaults, so
        // C++ prototypes come back labelled as declarations.
        init_grammar_cache();
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/a.cpp"),
            "void go(int v);\nvoid go(int v) { (void)v; }\n",
        )
        .unwrap();

        let idx = Index::load_or_build(dir.path(), &metadata_req());
        assert_eq!(idx.entries.len(), 1);
        drop(idx);

        // Stamp the previous schema version onto the existing index.
        let db = Database::create(cache_path_for(dir.path())).unwrap();
        {
            let write_txn = db.begin_write().unwrap();
            {
                let mut table = write_txn.open_table(META_TABLE).unwrap();
                let _ = table.insert("version", 8u32.to_le_bytes().as_slice());
            }
            write_txn.commit().unwrap();
        }
        drop(db);

        let rebuilt = Index::load_or_build(dir.path(), &metadata_req());
        let symbols = &rebuilt
            .entries
            .get(&PathBuf::from("src/a.cpp"))
            .expect("file must be reindexed")
            .symbols;
        let roles: Vec<SymbolRole> = symbols.iter().map(|s| s.role).collect();
        assert_eq!(
            roles,
            vec![SymbolRole::Declaration, SymbolRole::Definition],
            "{symbols:#?}"
        );

        // And the rebuilt index is written back at the current version.
        drop(rebuilt);
        let reopened = Index::load_or_build(dir.path(), &metadata_req());
        assert_eq!(
            reopened
                .entries
                .get(&PathBuf::from("src/a.cpp"))
                .unwrap()
                .symbols[0]
                .role,
            SymbolRole::Declaration
        );
    }

    #[test]
    fn test_symbols_persisted_to_redb() {
        let (dir, idx) = build_temp_index(&[(
            "src/main.rs",
            "pub fn foo(x: i32) -> bool { true }\nstruct Bar;\n",
        )]);

        let syms = &idx
            .entries
            .get(&PathBuf::from("src/main.rs"))
            .unwrap()
            .symbols;
        assert!(
            syms.iter()
                .any(|s| s.name == "foo" && s.kind == SymbolKind::Fn)
        );
        assert!(
            syms.iter()
                .any(|s| s.name == "Bar" && s.kind == SymbolKind::Struct)
        );
        drop(idx);

        // Reload and verify symbols survive the roundtrip through redb + bincode
        let idx2 = Index::load_or_build(dir.path(), &metadata_req());
        let syms2 = &idx2
            .entries
            .get(&PathBuf::from("src/main.rs"))
            .unwrap()
            .symbols;
        assert!(
            syms2
                .iter()
                .any(|s| s.name == "foo" && s.kind == SymbolKind::Fn)
        );
        assert!(
            syms2
                .iter()
                .any(|s| s.name == "Bar" && s.kind == SymbolKind::Struct)
        );
    }
}
