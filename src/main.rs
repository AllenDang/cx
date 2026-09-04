mod index;
mod lang;
mod output;
mod query;
mod language;
mod util;

use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Parser)]
#[command(name = "cx", version, about = "Semantic code navigation for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Project root (default: git root from cwd, then cwd)
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    /// Emit JSON instead of TOON
    #[arg(long, global = true)]
    json: bool,

    /// Max number of results to return (overrides per-command default)
    #[arg(long, global = true)]
    limit: Option<usize>,

    /// Skip the first N results
    #[arg(long, global = true, default_value = "0")]
    offset: usize,

    /// Return all results (bypass default limit)
    #[arg(long, global = true, conflicts_with = "limit")]
    all: bool,

    /// Exclude test files and test symbols from results
    #[arg(long, global = true)]
    no_tests: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Table of contents — symbols + ranges + signatures for a file, or symbol names for a directory
    #[command(alias = "o")]
    Overview {
        /// File or directory to summarize
        path: PathBuf,
        /// Show full per-file overview (name, kind, range, signature) for directories
        #[arg(long)]
        full: bool,
    },
    /// Search symbols across project
    #[command(alias = "s")]
    Symbols {
        /// Filter to a specific file
        #[arg(long)]
        file: Option<PathBuf>,
        /// Glob pattern to match symbol names
        #[arg(long)]
        name: Option<String>,
        /// Filter by symbol kind
        #[arg(long)]
        kind: Option<index::SymbolKind>,
        /// Filter by symbol role (definition, declaration, heading, unknown)
        #[arg(long)]
        role: Option<index::SymbolRole>,
        /// List distinct symbol kinds with counts
        #[arg(long)]
        kinds: bool,
    },
    /// Get a function/type body without reading the whole file
    #[command(alias = "d")]
    Definition {
        /// Symbol name to look up
        #[arg(long)]
        name: String,
        /// Disambiguate by source file
        #[arg(long)]
        from: Option<PathBuf>,
        /// Filter by symbol kind
        #[arg(long)]
        kind: Option<index::SymbolKind>,
        /// Filter by symbol role (e.g. --role declaration for a C++ prototype)
        #[arg(long)]
        role: Option<index::SymbolRole>,
        /// Max lines for body output (default 200)
        #[arg(long, default_value = "200")]
        max_lines: usize,
    },
    /// Find all usages of a symbol across the project
    #[command(alias = "r")]
    References {
        /// Symbol name to find
        #[arg(long)]
        name: String,
        /// Limit search to a specific file
        #[arg(long)]
        file: Option<PathBuf>,
        /// Show exact reference lines with source context
        #[arg(long)]
        context: bool,
    },
    /// Manage language grammars
    Lang {
        #[command(subcommand)]
        action: LangAction,
    },
    /// Print the agent skill file to stdout
    Skill,
    /// Manage the index cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Subcommand)]
enum LangAction {
    /// Download and install language grammars
    Add {
        /// Language names (e.g. rust typescript python)
        languages: Vec<String>,
    },
    /// Remove installed language grammars
    Remove {
        /// Language names to remove
        languages: Vec<String>,
    },
    /// List supported languages and their install status
    List,
}

#[derive(Subcommand)]
enum CacheAction {
    /// Print the index cache path for the current project
    Path,
    /// Delete the cached index for the current project
    Clean,
}

/// Derive the project root as a single canonical identity (roadmap §4.1).
/// Priority:
/// 1. Explicit --root flag
/// 2. Walk up from a path argument to find .git
/// 3. Walk up from CWD
///
/// The result is always symlink-resolved so `/tmp/p` and `/private/tmp/p`, or a
/// symlinked checkout and its target, name one project and one index.
fn resolve_root(explicit: &Option<PathBuf>, path_hint: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return util::path::canonical(p);
    }
    if let Some(hint) = path_hint {
        return util::path::canonical(&util::git::find_project_root(&util::path::canonical(hint)));
    }
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    util::path::canonical(&util::git::find_project_root(&util::path::canonical(&cwd)))
}

fn main() {
    // Use our own cache directory so grammar downloads survive crate version bumps.
    // See KNOWN_ISSUES.md for details.
    let config = tree_sitter_language_pack::PackConfig {
        cache_dir: Some(lang::grammar_cache_dir()),
        ..Default::default()
    };
    if let Err(e) = tree_sitter_language_pack::configure(&config) {
        eprintln!("cx: failed to configure grammar cache: {e}");
    }

    let cli = Cli::parse();

    let resolve_pagination = |default_limit: Option<usize>| -> query::Pagination {
        let limit = if cli.all {
            None
        } else {
            Some(cli.limit.unwrap_or_else(|| default_limit.unwrap_or(usize::MAX)))
        };
        let limit = limit.filter(|&n| n < usize::MAX);
        query::Pagination { limit, offset: cli.offset }
    };

    let exit_code = match cli.command {
        Commands::Overview { ref path, full } => {
            let root = resolve_root(&cli.root, Some(path));
            let idx = index::Index::load_or_build(&root);
            let abs = util::path::canonical(path);
            if abs.is_dir() {
                query::dir_overview(&idx, path, full, cli.no_tests, cli.json, &resolve_pagination(None))
            } else {
                let filters = query::Filters { file: Some(path), ..Default::default() };
                query::symbols(&idx, &filters, true, cli.json, &resolve_pagination(None))
            }
        }
        Commands::Symbols { ref file, ref name, kind, role, kinds } => {
            let root = resolve_root(&cli.root, file.as_deref());
            let idx = index::Index::load_or_build(&root);
            if kinds {
                query::kind_counts(&idx, file.as_deref(), cli.json)
            } else {
                let filters = query::Filters {
                    file: file.as_deref(),
                    name_glob: name.as_deref(),
                    kind,
                    role,
                };
                query::symbols(&idx, &filters, false, cli.json, &resolve_pagination(Some(100)))
            }
        }
        Commands::Definition { ref name, ref from, kind, role, max_lines } => {
            let root = resolve_root(&cli.root, from.as_deref());
            let idx = index::Index::load_or_build(&root);
            let default = if from.is_some() { None } else { Some(3) };
            let filters = query::Filters {
                file: from.as_deref(),
                kind,
                role,
                ..Default::default()
            };
            query::definition(&idx, name, &filters, max_lines, cli.json, &resolve_pagination(default))
        }
        Commands::References { ref name, ref file, context } => {
            let root = resolve_root(&cli.root, file.as_deref());
            let idx = index::Index::load_or_build(&root);
            query::references(&idx, name, file.as_deref(), context, cli.json, &resolve_pagination(Some(50)))
        }
        Commands::Lang { action } => {
            match action {
                LangAction::Add { languages } => lang::add(&languages),
                LangAction::Remove { languages } => lang::remove(&languages),
                LangAction::List => lang::list(),
            }
        }
        Commands::Skill => {
            print!("{}", include_str!("skill.md"));
            0
        }
        Commands::Cache { action } => {
            let root = resolve_root(&cli.root, None);
            let path = index::cache_path_for(&root);
            match action {
                CacheAction::Path => {
                    println!("{}", path.display());
                    0
                }
                CacheAction::Clean => {
                    if path.exists() {
                        if let Err(e) = fs::remove_file(&path) {
                            eprintln!("cx: failed to remove cache: {e}");
                            1
                        } else {
                            eprintln!("cx: removed {}", path.display());
                            0
                        }
                    } else {
                        eprintln!("cx: no cached index for this project");
                        0
                    }
                }
            }
        }
    };

    process::exit(exit_code);
}
