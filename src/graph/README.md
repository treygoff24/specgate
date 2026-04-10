# graph/

This module builds the dependency graph that the rest of Specgate reasons over.

It provides deterministic, project-wide graph construction on top of parser and resolver output:

- source file discovery (`discovery.rs`)
- module membership lookup (`DependencyGraph::module_of_file`, `files_in_module`)
- first-party dependency edges with typed kinds (`EdgeKind`)
- SCC/cycle helpers (`strongly_connected_components`, `find_cycles`)
- diff-mode impact expansion (`affected_modules`)
