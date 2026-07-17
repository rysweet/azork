CREATE (:Layer {slug: 'repo-surface', name: 'Repository Surface', diagram_type: 'flowchart TD'});
CREATE (:Layer {slug: 'ast-lsp-bindings', name: 'AST+LSP Symbol Bindings', diagram_type: 'flowchart LR'});
CREATE (:Layer {slug: 'compile-deps', name: 'Compile-time Dependencies', diagram_type: 'DOT digraph'});
CREATE (:Layer {slug: 'runtime-topology', name: 'Runtime Topology', diagram_type: 'DOT digraph'});
CREATE (:Layer {slug: 'api-contracts', name: 'API Contracts', diagram_type: 'flowchart TD'});
CREATE (:Layer {slug: 'data-flow', name: 'Data Flow', diagram_type: 'flowchart LR'});
CREATE (:Layer {slug: 'service-components', name: 'Service Component Architecture', diagram_type: 'graph TD'});
CREATE (:Layer {slug: 'user-journeys', name: 'User Journey Scenarios', diagram_type: 'sequenceDiagram'});
