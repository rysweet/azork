// Ready-to-run example queries against the Code Atlas graph.
// 1. All HTTP + CLI endpoints exposed by each service.
MATCH (s:Service)-[:Exposes]->(e:Endpoint) RETURN s.name, e.kind, e.path, e.handler ORDER BY s.name, e.path;
// 2. Which services each user journey traverses.
MATCH (j:Journey)-[:Traverses]->(s:Service) RETURN j.name, collect(s.name) ORDER BY j.name;
// 3. Service dependency edges (module coupling).
MATCH (a:Service)-[:DependsOn]->(b:Service) RETURN a.name, b.name ORDER BY a.name, b.name;
// 4. Open atlas bug-hunt findings by severity and layer.
MATCH (b:Bug)-[:Found]->(l:Layer) RETURN b.severity, l.slug, b.title, b.file, b.line ORDER BY b.severity, l.slug;
// 5. Layers and the services they cover.
MATCH (l:Layer)-[:Covers]->(s:Service) RETURN l.slug, collect(s.name) ORDER BY l.slug;
