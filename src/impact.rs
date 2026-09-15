//! Query-local reverse graph and bounded BFS over exact sites and evidence states.
use crate::index::{Index, SymbolRole};
use crate::language::{CallFacts, CallSite};
use crate::output::ErrorCode;
use crate::relation_index::{Candidate, Coverage, CoverageReason, DefinitionSiteId, RelationIndex};
use crate::relations::ResolutionLevel;
use crate::snapshot::{Sources, relative_file};
use crate::task::{Failure, Report};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct NodeId {
    pub file: PathBuf,
    pub language: String,
    pub range: (usize, usize),
    pub kind: &'static str,
}
impl NodeId {
    fn symbol(site: &DefinitionSiteId) -> Self {
        Self {
            file: site.file.clone(),
            language: site.language.clone(),
            range: site.range,
            kind: "symbol",
        }
    }
}
#[derive(Clone, Serialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub qualified: Option<String>,
    pub role: String,
    pub line: usize,
    pub is_test: bool,
}
#[derive(Clone, Serialize)]
pub struct Edge {
    pub caller: NodeId,
    pub caller_name: String,
    pub callee: NodeId,
    pub callee_name: String,
    pub file: PathBuf,
    pub line: usize,
    pub byte_range: (usize, usize),
    pub resolution: String,
    pub possible_reason: Option<&'static str>,
}
#[derive(Clone, Serialize)]
pub struct Witness {
    pub depth: usize,
    pub path: Vec<Edge>,
}
#[derive(Serialize)]
pub struct Row {
    pub symbol: Node,
    pub depth: usize,
    pub depth_exact: bool,
    pub supported: Option<Witness>,
    pub possible: Option<Witness>,
}
#[derive(Clone, Serialize)]
struct Frontier {
    caller: Node,
    file: PathBuf,
    line: usize,
    byte_range: (usize, usize),
    reason: &'static str,
    candidates: Vec<NodeId>,
    candidate_count: usize,
    candidates_omitted: usize,
}
struct Unresolved {
    frontier: Frontier,
    targets: Vec<NodeId>,
}

#[derive(Clone)]
pub struct Options {
    pub name: String,
    pub scope: Option<String>,
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
    pub byte_offset: Option<usize>,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub snapshot: Option<String>,
    pub no_tests: bool,
}

pub struct Graph<'a> {
    index: &'a Index,
    sources: &'a Sources,
    relations: RelationIndex<'a>,
    parsed: BTreeMap<PathBuf, CallFacts>,
    parsed_reasons: BTreeMap<PathBuf, Option<CoverageReason>>,
    query_files: BTreeSet<PathBuf>,
    nodes: BTreeMap<NodeId, Node>,
    incoming: BTreeMap<NodeId, Vec<Edge>>,
    unresolved: Vec<Unresolved>,
    expanded_names: BTreeSet<String>,
    coverage: Coverage,
    examined: usize,
    edge_limit: bool,
    candidate_universe_verified: bool,
}
impl<'a> Graph<'a> {
    pub fn new(index: &'a Index, sources: &'a Sources) -> Self {
        let relations = RelationIndex::from_cached(index);
        let mut coverage = relations.coverage.clone();
        coverage.scope = "indexed_candidates; reverse_frontier_names";
        let candidate_universe_verified =
            index.freshness.mode == "verified" || index.freshness.mode == "snapshot";
        if !candidate_universe_verified {
            coverage.complete_within_model = false;
        }
        Self {
            index,
            sources,
            relations,
            parsed: BTreeMap::new(),
            parsed_reasons: BTreeMap::new(),
            query_files: BTreeSet::new(),
            nodes: BTreeMap::new(),
            incoming: BTreeMap::new(),
            unresolved: Vec::new(),
            expanded_names: BTreeSet::new(),
            coverage,
            examined: 0,
            edge_limit: false,
            candidate_universe_verified,
        }
    }
    fn node(&self, candidate: &Candidate<'_>) -> Node {
        Node {
            id: NodeId::symbol(&candidate.id),
            name: candidate.symbol.name.clone(),
            qualified: candidate.symbol.qualified_name.clone(),
            role: candidate.symbol.role.as_str().into(),
            line: self.index.entries[&candidate.id.file]
                .task
                .line(candidate.id.range.0),
            is_test: candidate.symbol.is_test || crate::query::is_test_file(&candidate.id.file),
        }
    }
    fn caller(&self, file: &Path, site: &CallSite) -> Node {
        let data = &self.index.entries[file];
        if let Some(symbol) = crate::relations::owner(&data.symbols, site) {
            return Node {
                id: NodeId {
                    file: file.into(),
                    language: data.meta.language.clone(),
                    range: symbol.byte_range,
                    kind: "symbol",
                },
                name: symbol.name.clone(),
                qualified: symbol.qualified_name.clone(),
                role: symbol.role.as_str().into(),
                line: data.task.line(symbol.byte_range.0),
                is_test: symbol.is_test || crate::query::is_test_file(file),
            };
        }
        let (range, kind) = site.owner_range.map_or(((0, 0), "file_scope"), |range| {
            (
                range,
                if site.anonymous_owner {
                    "anonymous"
                } else {
                    "unindexed_callable"
                },
            )
        });
        Node {
            id: NodeId {
                file: file.into(),
                language: data.meta.language.clone(),
                range,
                kind,
            },
            name: format!("({kind}@{})", range.0),
            qualified: None,
            role: kind.into(),
            line: data.task.line(range.0),
            is_test: crate::query::is_test_file(file),
        }
    }
    fn parse(&mut self, file: &Path) {
        if !self.query_files.insert(file.into()) {
            return;
        }
        if !self.parsed.contains_key(file) {
            let data = &self.index.entries[file];
            let bytes = self.sources.files.get(file).cloned().or_else(|| {
                crate::index::read_beneath(&self.index.root, file)
                    .ok()
                    .map(|(b, _)| b)
            });
            let valid = bytes
                .as_ref()
                .is_some_and(|bytes| crate::index::content_hash(bytes) == data.meta.content_hash);
            let (facts, reason) = if !valid {
                (
                    CallFacts {
                        sites: vec![],
                        has_parse_errors: false,
                        unsupported_calls: 0,
                    },
                    Some(CoverageReason::ContentChanged),
                )
            } else {
                match data.task.calls.clone() {
                    Some(cached) => {
                        let reason = if cached.has_parse_errors {
                            Some(CoverageReason::ParseError)
                        } else if cached.unsupported_calls > 0 {
                            Some(CoverageReason::UnsupportedCallForm)
                        } else {
                            None
                        };
                        (cached.expand(), reason)
                    }
                    None => (
                        CallFacts {
                            sites: vec![],
                            has_parse_errors: false,
                            unsupported_calls: 0,
                        },
                        Some(CoverageReason::UnsupportedLanguage),
                    ),
                }
            };
            self.parsed.insert(file.into(), facts);
            self.parsed_reasons.insert(file.into(), reason);
        }
        self.coverage.files_analyzed += 1;
        if let Some(reason) = self.parsed_reasons[file] {
            self.coverage.record(file, reason);
        }
    }
    /// Cache each matching file parse and each name's resolver work. Parsing an
    /// unrelated recovery tree never creates a strong path: relevant degraded
    /// name coverage marks otherwise unique associations as possible only.
    fn expand_name(&mut self, name: &str, max_edges: usize) {
        if !self.expanded_names.insert(name.into()) {
            return;
        }
        let files: Vec<_> = self
            .index
            .entries
            .iter()
            .filter(|(_, data)| {
                data.task
                    .calls
                    .as_ref()
                    .is_some_and(|facts| facts.contains_name(name))
            })
            .map(|(path, _)| path.clone())
            .collect();
        for file in &files {
            self.parse(file);
        }
        let degraded: BTreeSet<_> = files
            .iter()
            .filter(|p| self.parsed[*p].has_parse_errors)
            .map(|p| self.index.entries[p].meta.language.clone())
            .collect();
        let sites: Vec<_> = files
            .iter()
            .flat_map(|p| {
                self.parsed[p]
                    .sites
                    .iter()
                    .filter(|s| s.name == name)
                    .map(|s| (p.clone(), s.clone()))
            })
            .collect();
        for (file, site) in sites {
            if self.examined >= max_edges {
                self.edge_limit = true;
                break;
            }
            self.examined += 1;
            let caller = self.caller(&file, &site);
            self.nodes
                .entry(caller.id.clone())
                .or_insert_with(|| caller.clone());
            let data = &self.index.entries[&file];
            let resolution = self.relations.resolve(
                &file,
                &data.meta.language,
                crate::relations::owner(&data.symbols, &site),
                &site,
            );
            if let Some(target) = resolution.to {
                let target_node = self.node(target);
                self.nodes
                    .entry(target_node.id.clone())
                    .or_insert_with(|| target_node.clone());
                let possible_reason = if degraded.contains(&data.meta.language) {
                    Some("partial_name_coverage")
                } else if resolution.level == ResolutionLevel::Syntax {
                    Some("syntax_only_unique_target")
                } else {
                    None
                };
                self.incoming
                    .entry(target_node.id.clone())
                    .or_default()
                    .push(Edge {
                        caller: caller.id,
                        caller_name: caller.qualified.unwrap_or(caller.name),
                        callee: target_node.id,
                        callee_name: target.label(),
                        file,
                        line: site.line,
                        byte_range: (site.byte_offset, site.byte_end),
                        resolution: resolution.level.as_str().into(),
                        possible_reason,
                    });
            } else {
                let targets: Vec<_> = resolution
                    .candidates
                    .iter()
                    .map(|c| NodeId::symbol(&c.id))
                    .collect();
                if targets.is_empty() {
                    continue;
                }
                self.unresolved.push(Unresolved {
                    frontier: Frontier {
                        caller,
                        file,
                        line: site.line,
                        byte_range: (site.byte_offset, site.byte_end),
                        reason: "unresolved_target",
                        candidates: targets.iter().take(20).cloned().collect(),
                        candidate_count: targets.len(),
                        candidates_omitted: targets.len().saturating_sub(20),
                    },
                    targets,
                });
            }
        }
        for edges in self.incoming.values_mut() {
            edges.sort_by(|a, b| {
                a.caller
                    .cmp(&b.caller)
                    .then(a.file.cmp(&b.file))
                    .then(a.byte_range.cmp(&b.byte_range))
            });
        }
    }
    fn roots(&self, opts: &Options) -> Result<Vec<Node>, Failure> {
        let file = opts
            .file
            .as_deref()
            .map(|p| relative_file(&self.index.root, p))
            .transpose()?;
        if let Some(path) = &file
            && !self.index.entries.contains_key(path)
        {
            return Err(Failure::new(
                ErrorCode::FileNotIndexed,
                format!("{} is not indexed", path.display()),
            ));
        }
        let mut matches: Vec<_> = self
            .relations
            .named(&opts.name)
            .iter()
            .filter(|c| {
                let scoped = opts.scope.as_deref().is_none_or(|s| {
                    c.symbol
                        .qualified_name
                        .as_deref()
                        .is_some_and(|q| crate::util::glob::glob_match(s, q))
                });
                let site_matches = |site: &DefinitionSiteId| {
                    file.as_ref().is_none_or(|p| *p == site.file)
                        && opts.line.is_none_or(|l| {
                            self.index.entries[&site.file].task.line(site.range.0) == l
                        })
                        && opts.byte_offset.is_none_or(|b| b == site.range.0)
                };
                scoped && (site_matches(&c.id) || c.declarations.iter().any(site_matches))
            })
            .collect();
        if matches
            .iter()
            .any(|c| c.symbol.role == SymbolRole::Definition)
        {
            matches.retain(|c| c.symbol.role == SymbolRole::Definition);
        }
        Ok(matches.iter().map(|c| self.node(c)).collect())
    }
    pub fn impact(&mut self, opts: &Options) -> Report<Row> {
        // Multi-root clients (changes) share immutable parses/candidates, not a
        // previous root's budget, incomplete adjacency or witness state.
        self.nodes.clear();
        self.incoming.clear();
        self.unresolved.clear();
        self.expanded_names.clear();
        self.query_files.clear();
        self.examined = 0;
        self.edge_limit = false;
        self.coverage = self.relations.coverage.clone();
        self.coverage.scope = "indexed_candidates; reverse_frontier_names";
        if !self.candidate_universe_verified {
            self.coverage.complete_within_model = false;
        }
        if let Err(e) = self.sources.check(opts.snapshot.as_deref()) {
            return Report::failure(e);
        }
        let roots = match self.roots(opts) {
            Ok(roots) => roots,
            Err(e) => return Report::failure(e),
        };
        if roots.len() != 1 {
            let mut report = Report::failure(Failure::new(
                if roots.is_empty() {
                    ErrorCode::SubjectNotFound
                } else {
                    ErrorCode::SubjectAmbiguous
                },
                if roots.is_empty() {
                    "subject not found"
                } else {
                    "choose one site using --file and --line or --byte-offset"
                },
            ));
            report.analysis = json!({"root":null,"root_candidates":roots.iter().take(20).collect::<Vec<_>>(),"root_candidate_count":roots.len(),"snapshot":self.sources.id});
            return report;
        }
        let root = roots[0].clone();
        let root_bytes = self.sources.files.get(&root.id.file).cloned().or_else(|| {
            crate::index::read_beneath(&self.index.root, &root.id.file)
                .ok()
                .map(|(b, _)| b)
        });
        if !root_bytes.as_ref().is_some_and(|bytes| {
            crate::index::content_hash(bytes) == self.index.entries[&root.id.file].meta.content_hash
        }) {
            let mut report = Report::failure(Failure::new(
                ErrorCode::ContentChanged,
                format!(
                    "{} changed after indexing; refresh and retry",
                    root.id.file.display()
                ),
            ));
            report.analysis = json!({"root":root,"snapshot":self.sources.id});
            return report;
        }
        if !crate::language::supports_calls(&root.id.language) {
            let mut report = Report::failure(Failure::new(
                ErrorCode::UnsupportedAnalysis,
                format!("{} has no call model", root.id.language),
            ));
            report.analysis = json!({"root":root,"snapshot":self.sources.id});
            return report;
        }
        self.nodes.insert(root.id.clone(), root.clone());
        let mut states: BTreeMap<(NodeId, bool), Witness> = BTreeMap::new();
        states.insert(
            (root.id.clone(), false),
            Witness {
                depth: 0,
                path: vec![],
            },
        );
        let mut queue = VecDeque::from([(root.id.clone(), false)]);
        let mut discovered = BTreeSet::from([root.id.clone()]);
        let mut stops = BTreeSet::new();
        let mut frontier = Vec::new();
        let mut frontier_sites = BTreeSet::new();
        while let Some((id, possible)) = queue.pop_front() {
            let witness = states[&(id.clone(), possible)].clone();
            let node = self.nodes[&id].clone();
            if id.kind != "symbol" {
                stops.insert("unmodelled_owner_bindings");
                continue;
            }
            self.expand_name(&node.name, opts.max_edges);
            for item in &self.unresolved {
                if item.targets.contains(&id)
                    && frontier_sites.insert((item.frontier.file.clone(), item.frontier.byte_range))
                    && frontier.len() < 20
                {
                    frontier.push(item.frontier.clone());
                }
            }
            for edge in self.incoming.get(&id).cloned().unwrap_or_default() {
                if edge.caller == root.id {
                    continue;
                }
                let next_possible = possible || edge.possible_reason.is_some();
                let key = (edge.caller.clone(), next_possible);
                if states.contains_key(&key) {
                    continue;
                }
                if witness.depth >= opts.max_depth {
                    stops.insert("max_depth");
                    continue;
                }
                if !discovered.contains(&edge.caller) && discovered.len() >= opts.max_nodes {
                    stops.insert("max_nodes");
                    continue;
                }
                discovered.insert(edge.caller.clone());
                let mut path = vec![edge];
                path.extend(witness.path.clone());
                states.insert(
                    key.clone(),
                    Witness {
                        depth: witness.depth + 1,
                        path,
                    },
                );
                queue.push_back(key);
            }
        }
        if self.edge_limit {
            stops.insert("max_edges");
        }
        let complete = stops.is_empty() && self.coverage.complete_within_model;
        let mut rows = Vec::new();
        let mut filtered = 0;
        for id in discovered.into_iter().filter(|id| *id != root.id) {
            let supported = states.get(&(id.clone(), false)).cloned();
            let possible = states.get(&(id.clone(), true)).cloned();
            let depth = supported
                .iter()
                .chain(possible.iter())
                .map(|w| w.depth)
                .min()
                .unwrap();
            let symbol = self.nodes[&id].clone();
            if opts.no_tests && symbol.is_test {
                filtered += 1;
                continue;
            }
            rows.push(Row {
                symbol,
                depth,
                depth_exact: complete,
                supported,
                possible,
            });
        }
        rows.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.symbol.id.cmp(&b.symbol.id)));
        let supported_count = rows.iter().filter(|r| r.supported.is_some()).count();
        Report {
            analysis: json!({"model":"unique_target_reverse_calls_v1", "root":root,"root_candidates":[],
            "snapshot":self.sources.id,"coverage":self.coverage,"traversal_complete":stops.is_empty(),
            "stop_reasons":stops,"edges_examined":self.examined,"nodes_visited":states.keys().map(|(id,_)|id).collect::<BTreeSet<_>>().len(),
            "candidate_universe_verified":self.candidate_universe_verified,"supported_count":supported_count,"possible_only_count":rows.len()-supported_count,
            "frontier":frontier,"frontier_count":frontier_sites.len(),"frontier_omitted":frontier_sites.len().saturating_sub(20),
            "filtered_tests":filtered,
            "limits":{"max_depth":opts.max_depth,"max_nodes":opts.max_nodes,"max_edges":opts.max_edges},
            "limitations":"Static indexed-candidate evidence, not compiler/runtime impact. Supported and possible paths are separate. Ambiguous/receiver/macro edges are not traversed. Partial depths are witnessed upper bounds."}),
            rows,
            complete,
            warnings: if complete {
                vec![]
            } else {
                vec!["Partial analysis: inspect coverage and traversal stop reasons; an empty result is not a safety verdict".into()]
            },
            error: None,
        }
    }
}

pub fn run(index: &Index, options: &Options) -> Report<Row> {
    let sources = Sources::index_only(index);
    Graph::new(index, &sources).impact(options)
}

pub fn compact(report: Report<Row>) -> Report<serde_json::Value> {
    let mut analysis = report.analysis;
    if let Some(frontier) = analysis
        .get_mut("frontier")
        .and_then(serde_json::Value::as_array_mut)
    {
        *frontier = frontier.iter().take(5).map(|f| json!({"file":f["file"],"line":f["line"],"reason":f["reason"],
            "candidate_count":f["candidate_count"],"candidates_omitted":f["candidates_omitted"],
            "candidates":f["candidates"].as_array().map(|a|a.iter().take(5).collect::<Vec<_>>()).unwrap_or_default()})).collect();
    }
    analysis["detail"] = json!("compact");
    analysis["detail_omitted"] = json!(["dual_full_witness_paths", "extended_frontier_candidates"]);
    analysis["limitations"] = json!([
        "static_only",
        "no_type_or_runtime_resolution",
        "ambiguous_and_dynamic_edges_not_traversed",
        "partial_depth_is_upper_bound"
    ]);
    let rows = report.rows.into_iter().map(|row| {
        let (evidence,witness) = if let Some(supported)=row.supported.as_ref(){("supported",Some(supported))}
            else if let Some(possible)=row.possible.as_ref(){("possible",Some(possible))}else{("none",None)};
        let path=witness.map(|w|w.path.iter().map(|e|json!({"caller":e.caller_name,"callee":e.callee_name,
            "file":e.file,"line":e.line,"resolution":e.resolution,"possible_reason":e.possible_reason})).collect::<Vec<_>>()).unwrap_or_default();
        json!({"symbol":row.symbol,"depth":row.depth,"depth_exact":row.depth_exact,"evidence":evidence,
            "also_possible":row.possible.is_some()&&row.supported.is_some(),"witness":path})
    }).collect();
    Report {
        rows,
        analysis,
        complete: report.complete,
        warnings: report.warnings,
        error: report.error,
    }
}
