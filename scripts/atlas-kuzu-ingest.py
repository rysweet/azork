#!/usr/bin/env python3
"""Ingest the Code Atlas manifest into a Kuzu graph and emit per-entity .cypher.

Usage: atlas-kuzu-ingest.py [manifest.json] [kuzu_db_dir]
Reads docs/atlas/atlas-manifest.json by default; writes docs/atlas/cypher/atlas-*.cypher.
Requires the 'kuzu' package (see /home/azureuser/.venvs/atlas).
"""
import json
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ATLAS = ROOT / "docs" / "atlas"
CYPHER = ATLAS / "cypher"


def q(s: str) -> str:
    return "'" + str(s).replace("\\", "\\\\").replace("'", "\\'") + "'"


def exec_cypher(conn, text: str) -> None:
    """Execute ';'-delimited Cypher, ignoring blank lines and // comment lines.

    Comment lines are stripped per-statement rather than skipping the whole
    statement when it merely happens to start with a comment.
    """
    for raw in text.split(";"):
        stmt = "\n".join(
            line for line in raw.splitlines() if not line.strip().startswith("//")
        ).strip()
        if stmt:
            conn.execute(stmt + ";")


def main() -> int:
    manifest_path = Path(sys.argv[1]) if len(sys.argv) > 1 else ATLAS / "atlas-manifest.json"
    db_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else ROOT / ".kuzu-atlas"
    data = json.loads(manifest_path.read_text())

    # Emit portable per-entity .cypher (INSERT/CREATE statements).
    layers = "\n".join(
        f"CREATE (:Layer {{slug: {q(l['slug'])}, name: {q(l['name'])}, diagram_type: {q(l.get('diagram_type',''))}}});"
        for l in data.get("layers", [])
    )
    services = "\n".join(
        f"CREATE (:Service {{name: {q(s['name'])}, kind: {q(s.get('kind',''))}, path: {q(s.get('path',''))}}});"
        for s in data.get("services", [])
    )
    bugs = "\n".join(
        f"CREATE (:Bug {{id: {q(b['id'])}, title: {q(b['title'])}, layer: {q(b.get('layer',''))}, file: {q(b.get('file',''))}, line: {int(b.get('line',0))}, severity: {q(b.get('severity',''))}}});"
        for b in data.get("bugs", [])
    )
    rels = []
    for r in data.get("relationships", []):
        t, a, b = r["type"], r["from"], r["to"]
        if t == "Covers":
            rels.append(f"MATCH (l:Layer {{slug: {q(a)}}}), (s:Service {{name: {q(b)}}}) CREATE (l)-[:Covers]->(s);")
        elif t == "Exposes":
            rels.append(f"MATCH (s:Service {{name: {q(a)}}}), (e:Endpoint {{id: {q(b)}}}) CREATE (s)-[:Exposes]->(e);")
        elif t == "Traverses":
            rels.append(f"MATCH (j:Journey {{id: {q(a)}}}), (s:Service {{name: {q(b)}}}) CREATE (j)-[:Traverses]->(s);")
        elif t == "DependsOn":
            rels.append(f"MATCH (a:Service {{name: {q(a)}}}), (b:Service {{name: {q(b)}}}) CREATE (a)-[:DependsOn]->(b);")
        elif t == "Found":
            rels.append(f"MATCH (bug:Bug {{id: {q(a)}}}), (l:Layer {{slug: {q(b)}}}) CREATE (bug)-[:Found]->(l);")
    (CYPHER / "atlas-layers.cypher").write_text(layers + "\n")
    (CYPHER / "atlas-services.cypher").write_text(services + "\n")
    (CYPHER / "atlas-bugs.cypher").write_text((bugs + "\n") if bugs else "// no confirmed bugs at ingestion time\n")
    (CYPHER / "atlas-relationships.cypher").write_text("\n".join(rels) + "\n")

    # Live Kuzu ingestion (REQUIRED).
    import kuzu
    if db_dir.is_dir():
        shutil.rmtree(db_dir)
    elif db_dir.exists():
        db_dir.unlink()
    wal = db_dir.with_name(db_dir.name + ".wal")
    if wal.exists():
        wal.unlink()
    db = kuzu.Database(str(db_dir))
    conn = kuzu.Connection(db)
    exec_cypher(conn, (CYPHER / "schema.cypher").read_text())
    for e in data.get("endpoints", []):
        conn.execute(
            f"CREATE (:Endpoint {{id: {q(e['id'])}, kind: {q(e.get('kind',''))}, path: {q(e.get('path',''))}, handler: {q(e.get('handler',''))}}});"
        )
    for j in data.get("journeys", []):
        conn.execute(f"CREATE (:Journey {{id: {q(j['id'])}, name: {q(j['name'])}}});")
    exec_cypher(conn, (CYPHER / "atlas-layers.cypher").read_text())
    exec_cypher(conn, (CYPHER / "atlas-services.cypher").read_text())
    exec_cypher(conn, (CYPHER / "atlas-bugs.cypher").read_text())
    exec_cypher(conn, (CYPHER / "atlas-relationships.cypher").read_text())
    n_layers = conn.execute("MATCH (l:Layer) RETURN count(*);").get_next()[0]
    n_services = conn.execute("MATCH (s:Service) RETURN count(*);").get_next()[0]
    n_bugs = conn.execute("MATCH (b:Bug) RETURN count(*);").get_next()[0]
    print(f"Kuzu ingest OK: {n_layers} layers, {n_services} services, {n_bugs} bugs -> {db_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
