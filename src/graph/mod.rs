use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use miette::Diagnostic;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::parser::{self, FileAnalysis};
use crate::resolver::{ModuleResolver, ResolvedImport};
use crate::spec::{JestMockMode, SpecConfig};

pub mod discovery;

use discovery::{DiscoveryWarning, discover_source_files};

#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: PathBuf,
    /// Module this file belongs to (None if not covered by any spec).
    pub module_id: Option<String>,
    pub analysis: FileAnalysis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeKind {
    RuntimeImport,
    TypeOnlyImport,
    ReExport,
    Require,
    DynamicImport,
    JestMock,
}

/// Classification of a dependency edge by resolution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    Resolved,
    UnresolvedLiteral,
    UnresolvedDynamic,
    External,
}

impl EdgeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::UnresolvedLiteral => "unresolved_literal",
            Self::UnresolvedDynamic => "unresolved_dynamic",
            Self::External => "external",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyEdge {
    pub from: PathBuf,
    pub to: PathBuf,
    pub kind: EdgeKind,
    pub specifier: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub span_start: Option<u32>,
    pub span_end: Option<u32>,
    pub ignored_by_comment: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleComponent {
    pub files: Vec<PathBuf>,
    pub modules: Vec<String>,
}

impl CycleComponent {
    pub fn is_cycle(&self) -> bool {
        self.files.len() > 1
    }

    pub fn is_internal(&self) -> bool {
        self.modules.len() <= 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleScope {
    Internal,
    External,
    Both,
}

#[derive(Debug, Error, Diagnostic)]
pub enum GraphError {
    #[error(transparent)]
    Discovery {
        #[from]
        source: discovery::DiscoveryError,
    },
    #[error(transparent)]
    Parse {
        #[from]
        source: parser::ParserError,
    },
}

pub type Result<T> = std::result::Result<T, GraphError>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeRecord {
    from: PathBuf,
    to: PathBuf,
    kind: EdgeKind,
    specifier: String,
    line: Option<u32>,
    column: Option<u32>,
    span_start: Option<u32>,
    span_end: Option<u32>,
    ignored_by_comment: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeRequest {
    kind: EdgeKind,
    specifier: String,
    line: Option<u32>,
    column: Option<u32>,
    span_start: Option<u32>,
    span_end: Option<u32>,
    ignored_by_comment: bool,
}

/// Record of an import that could not be resolved to a first-party file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnresolvedImportRecord {
    /// File containing the import.
    pub from: PathBuf,
    /// Raw import specifier as written.
    pub specifier: String,
    /// Edge kind (e.g., DynamicImport or RuntimeImport).
    pub kind: EdgeKind,
    /// Source line number.
    pub line: Option<u32>,
    /// Whether the import resolved to a third-party/external package.
    pub is_external: bool,
    /// Whether the import was suppressed by a @specgate-ignore comment.
    pub ignored_by_comment: bool,
}

impl UnresolvedImportRecord {
    pub fn edge_type(&self) -> EdgeType {
        if self.is_external {
            EdgeType::External
        } else if self.kind == EdgeKind::DynamicImport {
            EdgeType::UnresolvedDynamic
        } else {
            EdgeType::UnresolvedLiteral
        }
    }
}

pub struct DependencyGraph {
    project_root: PathBuf,
    graph: DiGraph<FileNode, EdgeRecord>,
    file_index: BTreeMap<PathBuf, NodeIndex>,
    module_membership: BTreeMap<PathBuf, String>,
    reverse_module_edges: BTreeMap<String, BTreeSet<String>>,
    canonical_lookup_cache: RefCell<BTreeMap<PathBuf, PathBuf>>,
    discovery_warnings: Vec<DiscoveryWarning>,
    unresolved_imports: Vec<UnresolvedImportRecord>,
}

impl DependencyGraph {
    /// Build the project-level file dependency graph.
    pub fn build(
        project_root: &Path,
        resolver: &mut ModuleResolver,
        config: &SpecConfig,
    ) -> Result<Self> {
        let project_root =
            fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());

        let discovery = discover_source_files(&project_root, &config.exclude)?;
        let files = discovery.files;

        let mut graph = DiGraph::<FileNode, EdgeRecord>::new();
        let mut file_index = BTreeMap::new();
        let mut module_membership = BTreeMap::new();

        for file in files {
            let analysis = parser::parse_file(&file)?;
            let module_id = resolver
                .module_for_file(&file)
                .map(|module| module.to_string());
            if let Some(module) = &module_id {
                module_membership.insert(file.clone(), module.clone());
            }

            let node = FileNode {
                path: file.clone(),
                module_id,
                analysis,
            };

            let node_index = graph.add_node(node);
            file_index.insert(file, node_index);
        }

        let mut pending_edges = BTreeSet::new();
        let mut unresolved_imports = Vec::new();

        for (from_path, from_idx) in &file_index {
            let requests = {
                let node = graph
                    .node_weight(*from_idx)
                    .expect("node index should remain valid");
                edge_requests(&node.analysis, config)
            };

            for request in requests {
                let resolved = resolver.resolve(from_path, &request.specifier);
                match &resolved {
                    ResolvedImport::FirstParty { resolved_path, .. } => {
                        let Some(target_idx) = file_index.get(resolved_path) else {
                            continue;
                        };

                        let target_path = graph
                            .node_weight(*target_idx)
                            .expect("node index should remain valid")
                            .path
                            .clone();

                        pending_edges.insert(EdgeRecord {
                            from: from_path.clone(),
                            to: target_path,
                            kind: request.kind,
                            specifier: request.specifier.clone(),
                            line: request.line,
                            column: request.column,
                            span_start: request.span_start,
                            span_end: request.span_end,
                            ignored_by_comment: request.ignored_by_comment,
                        });
                    }
                    ResolvedImport::ThirdParty { .. } => {
                        unresolved_imports.push(UnresolvedImportRecord {
                            from: from_path.clone(),
                            specifier: request.specifier.clone(),
                            kind: request.kind,
                            line: request.line,
                            is_external: true,
                            ignored_by_comment: request.ignored_by_comment,
                        });
                    }
                    ResolvedImport::Unresolvable { .. } => {
                        unresolved_imports.push(UnresolvedImportRecord {
                            from: from_path.clone(),
                            specifier: request.specifier.clone(),
                            kind: request.kind,
                            line: request.line,
                            is_external: false,
                            ignored_by_comment: request.ignored_by_comment,
                        });
                    }
                }
            }
        }
        unresolved_imports.sort();

        for edge in pending_edges {
            let from_idx = *file_index
                .get(&edge.from)
                .expect("source path should exist in index");
            let to_idx = *file_index
                .get(&edge.to)
                .expect("target path should exist in index");
            graph.add_edge(from_idx, to_idx, edge);
        }

        let reverse_module_edges = reverse_module_dependency_edges(&graph);

        Ok(Self {
            project_root,
            graph,
            file_index,
            module_membership,
            reverse_module_edges,
            canonical_lookup_cache: RefCell::new(BTreeMap::new()),
            discovery_warnings: discovery.warnings,
            unresolved_imports,
        })
    }

    /// Number of files represented as nodes.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of first-party dependency edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Non-fatal discovery diagnostics collected while walking the filesystem.
    pub fn discovery_warnings(&self) -> &[DiscoveryWarning] {
        &self.discovery_warnings
    }

    /// All non-first-party import records (external + unresolvable), sorted deterministically.
    pub fn unresolved_imports(&self) -> &[UnresolvedImportRecord] {
        &self.unresolved_imports
    }

    /// Canonical project root used by this graph.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Return all graph files in deterministic path order.
    pub fn files(&self) -> Vec<&FileNode> {
        self.file_index
            .values()
            .filter_map(|idx| self.graph.node_weight(*idx))
            .collect()
    }

    /// Look up a file node by path.
    pub fn file(&self, path: &Path) -> Option<&FileNode> {
        if let Some(idx) = self.file_index.get(path) {
            return self.graph.node_weight(*idx);
        }

        let canonical = self.lookup_canonical_path(path);
        let idx = self.file_index.get(&canonical)?;
        self.graph.node_weight(*idx)
    }

    /// Get all file nodes belonging to a module, sorted by path.
    pub fn files_in_module(&self, module_id: &str) -> Vec<&FileNode> {
        self.file_index
            .iter()
            .filter_map(|(_path, idx)| {
                let node = self.graph.node_weight(*idx)?;
                if node.module_id.as_deref() == Some(module_id) {
                    Some(node)
                } else {
                    None
                }
            })
            .collect()
    }

    /// List all discovered module IDs in stable order.
    pub fn modules(&self) -> Vec<String> {
        self.module_membership
            .values()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Deterministic list of dependency edges.
    pub fn dependency_edges(&self) -> Vec<DependencyEdge> {
        let mut edges = self
            .graph
            .edge_references()
            .map(|edge_ref| DependencyEdge {
                from: edge_ref.weight().from.clone(),
                to: edge_ref.weight().to.clone(),
                kind: edge_ref.weight().kind,
                specifier: edge_ref.weight().specifier.clone(),
                line: edge_ref.weight().line,
                column: edge_ref.weight().column,
                span_start: edge_ref.weight().span_start,
                span_end: edge_ref.weight().span_end,
                ignored_by_comment: edge_ref.weight().ignored_by_comment,
            })
            .collect::<Vec<_>>();

        edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then_with(|| a.to.cmp(&b.to))
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.specifier.cmp(&b.specifier))
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.column.cmp(&b.column))
                .then_with(|| a.span_start.cmp(&b.span_start))
                .then_with(|| a.span_end.cmp(&b.span_end))
        });

        edges
    }

    /// Deterministic list of outgoing edges from a single file.
    pub fn dependencies_from(&self, path: &Path) -> Vec<DependencyEdge> {
        let node_idx = if let Some(idx) = self.file_index.get(path) {
            *idx
        } else {
            let canonical = self.lookup_canonical_path(path);
            let Some(idx) = self.file_index.get(&canonical) else {
                return Vec::new();
            };
            *idx
        };

        let mut edges = self
            .graph
            .edges(node_idx)
            .map(|edge_ref| DependencyEdge {
                from: edge_ref.weight().from.clone(),
                to: edge_ref.weight().to.clone(),
                kind: edge_ref.weight().kind,
                specifier: edge_ref.weight().specifier.clone(),
                line: edge_ref.weight().line,
                column: edge_ref.weight().column,
                span_start: edge_ref.weight().span_start,
                span_end: edge_ref.weight().span_end,
                ignored_by_comment: edge_ref.weight().ignored_by_comment,
            })
            .collect::<Vec<_>>();

        edges.sort_by(|a, b| {
            a.to.cmp(&b.to)
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.specifier.cmp(&b.specifier))
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.column.cmp(&b.column))
                .then_with(|| a.span_start.cmp(&b.span_start))
                .then_with(|| a.span_end.cmp(&b.span_end))
        });

        edges
    }

    /// Look up the module_id for a given file path.
    pub fn module_of_file(&self, path: &Path) -> Option<&str> {
        if let Some(module_id) = self.module_membership.get(path) {
            return Some(module_id.as_str());
        }

        let canonical = self.lookup_canonical_path(path);
        self.module_membership.get(&canonical).map(String::as_str)
    }

    /// Get all module IDs that have at least one file importing from the given module.
    pub fn importers_of_module(&self, module_id: &str) -> BTreeSet<String> {
        self.reverse_module_edges
            .get(module_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Return SCCs (Tarjan) in deterministic order.
    ///
    /// Components include singleton SCCs; use `find_cycles` to filter to cycles only.
    pub fn strongly_connected_components(&self) -> Vec<CycleComponent> {
        let mut components = tarjan_scc(&self.graph)
            .into_iter()
            .map(|indices| self.component_from_indices(indices))
            .collect::<Vec<_>>();

        components.sort_by(component_order_key);
        components
    }

    /// Find cyclic SCC components by scope.
    pub fn find_cycles(&self, scope: CycleScope) -> Vec<CycleComponent> {
        let mut cycles = self
            .strongly_connected_components()
            .into_iter()
            .filter(|component| component.is_cycle())
            .filter(|component| match scope {
                CycleScope::Internal => component.is_internal(),
                CycleScope::External => !component.is_internal(),
                CycleScope::Both => true,
            })
            .collect::<Vec<_>>();

        cycles.sort_by(component_order_key);
        cycles
    }

    /// Determine modules affected by changed files in diff mode.
    ///
    /// Includes modules containing changed files plus transitive importer modules.
    pub fn affected_modules(&self, changed_files: &[PathBuf]) -> BTreeSet<String> {
        let mut affected = BTreeSet::new();

        for file in changed_files {
            if let Some(module_id) = self.module_of_file(file) {
                affected.insert(module_id.to_string());
            }
        }

        if affected.is_empty() {
            return affected;
        }

        let mut queue = affected.iter().cloned().collect::<VecDeque<_>>();

        while let Some(module) = queue.pop_front() {
            if let Some(importers) = self.reverse_module_edges.get(&module) {
                for importer in importers {
                    if affected.insert(importer.clone()) {
                        queue.push_back(importer.clone());
                    }
                }
            }
        }

        affected
    }

    fn component_from_indices(&self, indices: Vec<NodeIndex>) -> CycleComponent {
        let mut files = indices
            .iter()
            .filter_map(|idx| self.graph.node_weight(*idx))
            .map(|node| node.path.clone())
            .collect::<Vec<_>>();
        files.sort();

        let modules = indices
            .iter()
            .filter_map(|idx| self.graph.node_weight(*idx))
            .filter_map(|node| node.module_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        CycleComponent { files, modules }
    }

    fn lookup_canonical_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            if self.file_index.contains_key(path) || self.module_membership.contains_key(path) {
                return path.to_path_buf();
            }
        } else {
            let joined = self.project_root.join(path);
            if self.file_index.contains_key(&joined) || self.module_membership.contains_key(&joined)
            {
                return joined;
            }
        }

        let cache_key = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_root.join(path)
        };

        if let Some(cached) = self.canonical_lookup_cache.borrow().get(&cache_key) {
            return cached.clone();
        }

        let canonical = canonicalize_for_graph(&self.project_root, path);
        self.canonical_lookup_cache
            .borrow_mut()
            .insert(cache_key, canonical.clone());
        canonical
    }
}

fn reverse_module_dependency_edges(
    graph: &DiGraph<FileNode, EdgeRecord>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut reverse = BTreeMap::<String, BTreeSet<String>>::new();

    for edge in graph.edge_references() {
        let Some(source_node) = graph.node_weight(edge.source()) else {
            continue;
        };
        let Some(target_node) = graph.node_weight(edge.target()) else {
            continue;
        };

        let (Some(source_module), Some(target_module)) =
            (&source_node.module_id, &target_node.module_id)
        else {
            continue;
        };

        if source_module == target_module {
            continue;
        }

        reverse
            .entry(target_module.clone())
            .or_default()
            .insert(source_module.clone());
    }

    reverse
}

fn edge_requests(analysis: &FileAnalysis, config: &SpecConfig) -> Vec<EdgeRequest> {
    let mut requests = BTreeSet::new();

    for import in &analysis.imports {
        requests.insert(EdgeRequest {
            kind: if import.is_type_only {
                EdgeKind::TypeOnlyImport
            } else {
                EdgeKind::RuntimeImport
            },
            specifier: import.specifier.clone(),
            line: Some(import.line),
            column: Some(import.column),
            span_start: Some(import.span_start),
            span_end: Some(import.span_end),
            ignored_by_comment: import.ignore_comment.is_some(),
        });
    }

    for re_export in &analysis.re_exports {
        requests.insert(EdgeRequest {
            kind: EdgeKind::ReExport,
            specifier: re_export.specifier.clone(),
            line: Some(re_export.line),
            column: Some(re_export.column),
            span_start: None,
            span_end: None,
            ignored_by_comment: false,
        });
    }

    for require_call in &analysis.require_calls {
        requests.insert(EdgeRequest {
            kind: EdgeKind::Require,
            specifier: require_call.specifier.clone(),
            line: Some(require_call.line),
            column: Some(require_call.column),
            span_start: None,
            span_end: None,
            ignored_by_comment: false,
        });
    }

    for dynamic_import in &analysis.dynamic_imports {
        requests.insert(EdgeRequest {
            kind: EdgeKind::DynamicImport,
            specifier: dynamic_import.specifier.clone(),
            line: Some(dynamic_import.line),
            column: Some(dynamic_import.column),
            span_start: None,
            span_end: None,
            ignored_by_comment: false,
        });
    }

    if matches!(config.jest_mock_mode, JestMockMode::Enforce) {
        for jest_mock in &analysis.jest_mock_calls {
            requests.insert(EdgeRequest {
                kind: EdgeKind::JestMock,
                specifier: jest_mock.specifier.clone(),
                line: Some(jest_mock.line),
                column: Some(jest_mock.column),
                span_start: None,
                span_end: None,
                ignored_by_comment: false,
            });
        }
    }

    requests.into_iter().collect()
}

fn component_order_key(left: &CycleComponent, right: &CycleComponent) -> std::cmp::Ordering {
    left.files
        .first()
        .cmp(&right.files.first())
        .then_with(|| left.files.cmp(&right.files))
        .then_with(|| left.modules.cmp(&right.modules))
}

fn canonicalize_for_graph(project_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    } else {
        let absolute = project_root.join(path);
        fs::canonicalize(&absolute).unwrap_or(absolute)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::spec::{Boundaries, SpecConfig, SpecFile};

    use super::*;

    fn spec(module: &str, path: &str) -> SpecFile {
        SpecFile {
            version: "2.2".to_string(),
            module: module.to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some(path.to_string()),
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: None,
        }
    }

    #[test]
    fn build_graph_collects_expected_edge_kinds() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");

        fs::write(
            temp.path().join("src/main.ts"),
            r#"
import type { T } from "./types";
import { value } from "./runtime";
export * from "./reexported";

const dep = require("./required");

async function load() {
  await import("./dynamic");
}

jest.mock("./mocked");
console.log(value, dep);
"#,
        )
        .expect("write main");

        for file in [
            "types.ts",
            "runtime.ts",
            "reexported.ts",
            "required.ts",
            "dynamic.ts",
            "mocked.ts",
        ] {
            fs::write(temp.path().join("src").join(file), "export const x = 1;\n")
                .expect("write dependency file");
        }

        let specs = vec![spec("app", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");

        let config_warn = SpecConfig::default();
        let graph_warn =
            DependencyGraph::build(temp.path(), &mut resolver, &config_warn).expect("build graph");

        let kinds_warn = graph_warn
            .dependency_edges()
            .into_iter()
            .map(|edge| edge.kind)
            .collect::<BTreeSet<_>>();

        assert!(kinds_warn.contains(&EdgeKind::TypeOnlyImport));
        assert!(kinds_warn.contains(&EdgeKind::RuntimeImport));
        assert!(kinds_warn.contains(&EdgeKind::ReExport));
        assert!(kinds_warn.contains(&EdgeKind::Require));
        assert!(kinds_warn.contains(&EdgeKind::DynamicImport));
        assert!(!kinds_warn.contains(&EdgeKind::JestMock));

        resolver.clear_cache();

        let config_enforce = SpecConfig {
            jest_mock_mode: JestMockMode::Enforce,
            ..Default::default()
        };

        let graph_enforce = DependencyGraph::build(temp.path(), &mut resolver, &config_enforce)
            .expect("build graph enforce");

        let kinds_enforce = graph_enforce
            .dependency_edges()
            .into_iter()
            .map(|edge| edge.kind)
            .collect::<BTreeSet<_>>();

        assert!(kinds_enforce.contains(&EdgeKind::JestMock));
    }

    #[test]
    fn files_and_module_membership_are_deterministic() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/a")).expect("mkdir a");
        fs::create_dir_all(temp.path().join("src/b")).expect("mkdir b");

        fs::write(
            temp.path().join("src/b/second.ts"),
            "export const second = 2;\n",
        )
        .expect("write second");
        fs::write(
            temp.path().join("src/a/first.ts"),
            "export const first = 1;\n",
        )
        .expect("write first");

        let specs = vec![spec("alpha", "src/a/**/*"), spec("beta", "src/b/**/*")];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();
        let graph =
            DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph");

        let canonical_root = fs::canonicalize(temp.path()).expect("canonical root");

        let all_files = graph
            .files()
            .iter()
            .map(|node| {
                crate::deterministic::normalize_path(
                    node.path
                        .strip_prefix(&canonical_root)
                        .expect("file path should be under temp root"),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(all_files, vec!["src/a/first.ts", "src/b/second.ts"]);

        let alpha_files = graph
            .files_in_module("alpha")
            .into_iter()
            .map(|node| {
                crate::deterministic::normalize_path(
                    node.path
                        .strip_prefix(&canonical_root)
                        .expect("file path should be under temp root"),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(alpha_files, vec!["src/a/first.ts"]);
        assert_eq!(
            graph.module_of_file(&temp.path().join("src/a/first.ts")),
            Some("alpha")
        );
        assert_eq!(
            graph.modules(),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn affected_modules_include_transitive_importers() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/core")).expect("mkdir core");
        fs::create_dir_all(temp.path().join("src/feature")).expect("mkdir feature");
        fs::create_dir_all(temp.path().join("src/app")).expect("mkdir app");

        fs::write(temp.path().join("src/core/a.ts"), "export const a = 1;\n").expect("write core");
        fs::write(
            temp.path().join("src/feature/b.ts"),
            "import { a } from \"../core/a\"; export const b = a;\n",
        )
        .expect("write feature");
        fs::write(
            temp.path().join("src/app/c.ts"),
            "import { b } from \"../feature/b\"; export const c = b;\n",
        )
        .expect("write app");

        let specs = vec![
            spec("core", "src/core/**/*"),
            spec("feature", "src/feature/**/*"),
            spec("app", "src/app/**/*"),
        ];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();
        let graph =
            DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph");

        let affected = graph.affected_modules(&[temp.path().join("src/core/a.ts")]);
        assert_eq!(
            affected,
            BTreeSet::from(["app".to_string(), "core".to_string(), "feature".to_string(),])
        );

        let importers = graph.importers_of_module("core");
        assert_eq!(importers, BTreeSet::from(["feature".to_string()]));
    }

    #[test]
    fn cycle_helpers_filter_by_scope_and_stay_deterministic() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/alpha")).expect("mkdir alpha");
        fs::create_dir_all(temp.path().join("src/beta")).expect("mkdir beta");
        fs::create_dir_all(temp.path().join("src/gamma")).expect("mkdir gamma");

        // Internal cycle in alpha
        fs::write(
            temp.path().join("src/alpha/a.ts"),
            "import { b } from \"./b\"; export const a = b;\n",
        )
        .expect("write alpha a");
        fs::write(
            temp.path().join("src/alpha/b.ts"),
            "import { a } from \"./a\"; export const b = a;\n",
        )
        .expect("write alpha b");

        // External cycle between beta and gamma
        fs::write(
            temp.path().join("src/beta/x.ts"),
            "import { y } from \"../gamma/y\"; export const x = y;\n",
        )
        .expect("write beta x");
        fs::write(
            temp.path().join("src/gamma/y.ts"),
            "import { x } from \"../beta/x\"; export const y = x;\n",
        )
        .expect("write gamma y");

        let specs = vec![
            spec("alpha", "src/alpha/**/*"),
            spec("beta", "src/beta/**/*"),
            spec("gamma", "src/gamma/**/*"),
        ];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();
        let graph =
            DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph");

        let internal = graph.find_cycles(CycleScope::Internal);
        assert_eq!(internal.len(), 1);
        assert_eq!(internal[0].modules, vec!["alpha".to_string()]);

        let external = graph.find_cycles(CycleScope::External);
        assert_eq!(external.len(), 1);
        assert_eq!(
            external[0].modules,
            vec!["beta".to_string(), "gamma".to_string()]
        );

        let both = graph.find_cycles(CycleScope::Both);
        assert_eq!(both.len(), 2);

        let canonical_root = fs::canonicalize(temp.path()).expect("canonical root");

        let first_component_files = both[0]
            .files
            .iter()
            .map(|path| {
                crate::deterministic::normalize_path(
                    path.strip_prefix(&canonical_root)
                        .expect("path should be under temp root"),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            first_component_files,
            vec!["src/alpha/a.ts", "src/alpha/b.ts"]
        );
    }

    #[test]
    fn relative_paths_resolve_for_file_and_module_lookups() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/a")).expect("mkdir a");
        fs::write(
            temp.path().join("src/a/main.ts"),
            "export const main = 1;\n",
        )
        .expect("write");

        let specs = vec![spec("alpha", "src/a/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();
        let graph =
            DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph");

        let relative = Path::new("./src/a/../a/main.ts");
        assert!(graph.file(relative).is_some());
        assert_eq!(graph.module_of_file(relative), Some("alpha"));
    }

    #[test]
    fn dependencies_from_targets_single_file_and_stays_sorted() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");

        fs::write(
            temp.path().join("src/main.ts"),
            "import './z'; import './a'; import type { T } from './a';\n",
        )
        .expect("write main");
        fs::write(temp.path().join("src/a.ts"), "export type T = string;\n").expect("write a");
        fs::write(temp.path().join("src/z.ts"), "export const z = 1;\n").expect("write z");
        fs::write(
            temp.path().join("src/other.ts"),
            "export const other = 1;\n",
        )
        .expect("write other");

        let specs = vec![spec("app", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();
        let graph =
            DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph");

        let deps = graph.dependencies_from(&temp.path().join("src/main.ts"));
        assert_eq!(deps.len(), 3);

        let canonical_root = fs::canonicalize(temp.path()).expect("canonical root");
        let dep_targets = deps
            .iter()
            .map(|edge| {
                crate::deterministic::normalize_path(
                    edge.to
                        .strip_prefix(&canonical_root)
                        .expect("edge target should be under temp root"),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            dep_targets,
            vec!["src/a.ts", "src/a.ts", "src/z.ts"],
            "dependencies_from should only include outgoing edges for the requested file"
        );

        let unknown = graph.dependencies_from(Path::new("src/missing.ts"));
        assert!(unknown.is_empty());
    }

    #[test]
    fn preserves_distinct_import_occurrences_for_same_specifier() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");

        fs::write(
            temp.path().join("src/main.ts"),
            "// @specgate-ignore: temporary\nimport { value as ignored } from './dep';\nimport { value as enforced } from './dep';\nconsole.log(ignored, enforced);\n",
        )
        .expect("write main");
        fs::write(temp.path().join("src/dep.ts"), "export const value = 1;\n").expect("write dep");

        let specs = vec![spec("app", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();
        let graph =
            DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph");

        let deps = graph.dependencies_from(&temp.path().join("src/main.ts"));
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].specifier, "./dep");
        assert_eq!(deps[1].specifier, "./dep");
        assert!(deps.iter().any(|edge| edge.ignored_by_comment));
        assert!(deps.iter().any(|edge| !edge.ignored_by_comment));
    }

    #[test]
    fn self_loop_and_emptyish_graph_edge_cases_are_stable() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");
        fs::write(
            temp.path().join("src/self.ts"),
            "import { self } from './self'; export const selfRef = self;\n",
        )
        .expect("write self");

        let specs = vec![spec("solo", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();
        let graph =
            DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph");

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 1);
        assert_eq!(
            graph
                .dependencies_from(&temp.path().join("src/self.ts"))
                .len(),
            1
        );
        assert!(graph.find_cycles(CycleScope::Both).is_empty());

        assert!(
            graph
                .affected_modules(&[temp.path().join("src/does-not-exist.ts")])
                .is_empty()
        );
    }

    #[test]
    fn unresolved_import_record_carries_ignored_flag() {
        // An import preceded by @specgate-ignore that fails to resolve should
        // carry ignored_by_comment = true in UnresolvedImportRecord.
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");

        fs::write(
            temp.path().join("src/main.ts"),
            "// @specgate-ignore: temporary\nimport { x } from './missing-module';\nconsole.log(x);\n",
        )
        .expect("write main");

        let specs = vec![spec("app", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();
        let graph = DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build");

        let unresolved = graph.unresolved_imports();
        assert_eq!(unresolved.len(), 1, "should have one unresolved import");
        assert!(
            unresolved[0].ignored_by_comment,
            "ignored import should carry ignored_by_comment = true"
        );
    }
}
