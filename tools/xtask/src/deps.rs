//! `cargo xtask deps-check`: every workspace→workspace dependency edge must be listed in
//! `tools/xtask/dag.toml`, and no GPU crate may be reachable from the engine-side packages
//! (01-LAYOUT.md §2).

use crate::Verdict;
use crate::args;
use crate::walk;
use crate::workspace;
use cargo_metadata::{Metadata, PackageId};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs;
use std::path::Path;

const DAG_PATH: &str = "tools/xtask/dag.toml";

/// The allowed edges and the forbidden reachability rule, read from `dag.toml`.
#[derive(Debug, Default)]
struct Dag {
    allowed: BTreeMap<String, BTreeSet<String>>,
    forbidden_roots: Vec<String>,
    forbidden_crates: Vec<String>,
}

/// One package. `dir` is relative to the repository root (empty outside it).
#[derive(Debug)]
struct Node {
    name: String,
    dir: String,
    workspace: bool,
    deps: Vec<usize>,
}

#[derive(Debug)]
struct Graph {
    nodes: Vec<Node>,
}

pub fn run(args: &[String]) -> Result<Verdict, String> {
    args::reject_unknown(args, &[])?;
    let metadata = workspace::load()?;
    let root = workspace::root(&metadata);
    let dag = Dag::load(&root.join(DAG_PATH))?;
    let graph = Graph::from_metadata(&metadata, &root)?;

    let problems = violations(&dag, &graph);
    if problems.is_empty() {
        println!(
            "deps-check: {} workspace edges are all in {DAG_PATH}; no forbidden crate is reachable",
            graph.workspace_edges().len()
        );
        return Ok(Verdict::Pass);
    }
    for problem in &problems {
        eprintln!("deps-check: {problem}");
    }
    Ok(Verdict::Fail)
}

/// Every edge or reachability problem, as one line each.
fn violations(dag: &Dag, graph: &Graph) -> Vec<String> {
    let mut out: Vec<String> = graph
        .workspace_edges()
        .into_iter()
        .filter(|(from, to)| !dag.allowed.get(from).is_some_and(|set| set.contains(to)))
        .map(|(from, to)| format!("edge {from} -> {to} is not in {DAG_PATH}"))
        .collect();
    for (start, node) in graph.nodes.iter().enumerate() {
        let is_root = node.workspace
            && dag
                .forbidden_roots
                .iter()
                .any(|pattern| dir_matches(pattern, &node.dir));
        if !is_root {
            continue;
        }
        let forbidden = |candidate: &Node| {
            dag.forbidden_crates
                .iter()
                .any(|pattern| name_matches(pattern, &candidate.name))
        };
        if let Some(path) = graph.path_to(start, forbidden) {
            out.push(format!(
                "{} must not reach a GPU crate: {}",
                node.name,
                path.join(" -> ")
            ));
        }
    }
    out
}

impl Dag {
    fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let table: toml::Table =
            toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;

        let allowed = section(&table, "allowed")?
            .iter()
            .map(|(from, targets)| {
                let targets = strings(targets, &format!("allowed.{from}"))?;
                Ok((from.clone(), targets.into_iter().collect()))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let forbidden = section(&table, "forbidden")?;
        Ok(Self {
            allowed,
            forbidden_roots: strings(required(forbidden, "roots")?, "forbidden.roots")?,
            forbidden_crates: strings(required(forbidden, "crates")?, "forbidden.crates")?,
        })
    }
}

fn section<'a>(table: &'a toml::Table, key: &str) -> Result<&'a toml::Table, String> {
    table
        .get(key)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{DAG_PATH} needs a [{key}] table"))
}

fn required<'a>(table: &'a toml::Table, key: &str) -> Result<&'a toml::Value, String> {
    table
        .get(key)
        .ok_or_else(|| format!("{DAG_PATH} needs `{key}`"))
}

fn strings(value: &toml::Value, what: &str) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{what} must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{what} must hold strings"))
        })
        .collect()
}

impl Graph {
    fn from_metadata(metadata: &Metadata, root: &Path) -> Result<Self, String> {
        let resolve = metadata
            .resolve
            .as_ref()
            .ok_or("cargo metadata returned no dependency graph")?;
        let index: HashMap<&PackageId, usize> = metadata
            .packages
            .iter()
            .enumerate()
            .map(|(at, package)| (&package.id, at))
            .collect();
        let edges: HashMap<&PackageId, &Vec<PackageId>> = resolve
            .nodes
            .iter()
            .map(|node| (&node.id, &node.dependencies))
            .collect();
        let members: BTreeSet<&PackageId> = metadata.workspace_members.iter().collect();

        let nodes = metadata
            .packages
            .iter()
            .map(|package| {
                let deps: Vec<usize> = edges
                    .get(&package.id)
                    .into_iter()
                    .flat_map(|ids| ids.iter())
                    .filter_map(|id| index.get(id).copied())
                    .collect();
                let dir = workspace::package_dir(package)
                    .strip_prefix(root)
                    .map(|rel| walk::relative(rel, Path::new("")))
                    .unwrap_or_default();
                Node {
                    name: package.name.to_string(),
                    dir,
                    workspace: members.contains(&package.id),
                    deps,
                }
            })
            .collect();
        Ok(Self { nodes })
    }

    /// Distinct (from, to) pairs where both packages are in the workspace.
    fn workspace_edges(&self) -> BTreeSet<(String, String)> {
        self.nodes
            .iter()
            .filter(|from| from.workspace)
            .flat_map(|from| {
                from.deps
                    .iter()
                    .map(|&to| &self.nodes[to])
                    .filter(|to| to.workspace)
                    .map(|to| (from.name.clone(), to.name.clone()))
            })
            .collect()
    }

    /// Shortest dependency path from `start` to a node matching `is_target`, by name.
    fn path_to(&self, start: usize, is_target: impl Fn(&Node) -> bool) -> Option<Vec<String>> {
        let mut parent: Vec<Option<usize>> = vec![None; self.nodes.len()];
        let mut seen = vec![false; self.nodes.len()];
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        while let Some(current) = queue.pop_front() {
            for &next in &self.nodes[current].deps {
                if seen[next] {
                    continue;
                }
                seen[next] = true;
                parent[next] = Some(current);
                if is_target(&self.nodes[next]) {
                    let mut names = Vec::new();
                    let mut at = Some(next);
                    while let Some(node) = at {
                        names.push(self.nodes[node].name.clone());
                        at = parent[node];
                    }
                    names.reverse();
                    return Some(names);
                }
                queue.push_back(next);
            }
        }
        None
    }
}

/// `crates/base/**` matches the directory and everything under it; other patterns are exact.
fn dir_matches(pattern: &str, dir: &str) -> bool {
    match pattern.strip_suffix("/**") {
        Some(prefix) => {
            dir == prefix
                || dir
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
        None => dir == pattern,
    }
}

/// `wgpu*` is a prefix match; other patterns are exact.
fn name_matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == pattern,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, dir: &str, workspace: bool, deps: &[usize]) -> Node {
        Node {
            name: name.to_string(),
            dir: dir.to_string(),
            workspace,
            deps: deps.to_vec(),
        }
    }

    /// base → wgpu-core (external); sampler-core → sampler-ir (workspace).
    fn sample_graph(base_pulls_gpu: bool) -> Graph {
        let base_deps: &[usize] = if base_pulls_gpu { &[1] } else { &[] };
        Graph {
            nodes: vec![
                node("base", "crates/base/core", true, base_deps),
                node("wgpu-core", "", false, &[]),
                node("sampler-core", "crates/sampler-core", true, &[3]),
                node("sampler-ir", "crates/sampler-ir", true, &[]),
            ],
        }
    }

    fn dag(allowed: &[(&str, &[&str])], forbidden: bool) -> Dag {
        Dag {
            allowed: allowed
                .iter()
                .map(|(from, to)| (from.to_string(), to.iter().map(|s| s.to_string()).collect()))
                .collect(),
            forbidden_roots: if forbidden {
                vec!["crates/base/**".to_string()]
            } else {
                Vec::new()
            },
            forbidden_crates: vec!["wgpu*".to_string()],
        }
    }

    #[test]
    fn allowed_edges_pass_and_missing_ones_fail() {
        let graph = sample_graph(false);
        assert!(violations(&dag(&[("sampler-core", &["sampler-ir"])], false), &graph).is_empty());
        let problems = violations(&dag(&[], false), &graph);
        assert_eq!(
            problems,
            vec!["edge sampler-core -> sampler-ir is not in tools/xtask/dag.toml"]
        );
    }

    #[test]
    fn gpu_crate_reachable_from_a_root_is_reported_with_its_path() {
        let graph = sample_graph(true);
        let problems = violations(&dag(&[("sampler-core", &["sampler-ir"])], true), &graph);
        assert_eq!(
            problems,
            vec!["base must not reach a GPU crate: base -> wgpu-core"]
        );
    }

    #[test]
    fn glob_patterns_match_directories_and_names() {
        assert!(dir_matches("crates/base/**", "crates/base"));
        assert!(dir_matches("crates/base/**", "crates/base/inner"));
        assert!(!dir_matches("crates/base/**", "crates/baseline"));
        assert!(name_matches("wgpu*", "wgpu-core"));
        assert!(name_matches("ash", "ash"));
        assert!(!name_matches("ash", "ash-tools"));
    }
}
