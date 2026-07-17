// Code Atlas graph schema (Kuzu / OpenCypher-portable)
// Node tables
CREATE NODE TABLE IF NOT EXISTS Layer(slug STRING, name STRING, diagram_type STRING, PRIMARY KEY(slug));
CREATE NODE TABLE IF NOT EXISTS Service(name STRING, kind STRING, path STRING, PRIMARY KEY(name));
CREATE NODE TABLE IF NOT EXISTS Endpoint(id STRING, kind STRING, path STRING, handler STRING, PRIMARY KEY(id));
CREATE NODE TABLE IF NOT EXISTS Journey(id STRING, name STRING, PRIMARY KEY(id));
CREATE NODE TABLE IF NOT EXISTS Bug(id STRING, title STRING, layer STRING, file STRING, line INT64, severity STRING, PRIMARY KEY(id));
// Relationship tables
CREATE REL TABLE IF NOT EXISTS Covers(FROM Layer TO Service);
CREATE REL TABLE IF NOT EXISTS Exposes(FROM Service TO Endpoint);
CREATE REL TABLE IF NOT EXISTS Traverses(FROM Journey TO Service);
CREATE REL TABLE IF NOT EXISTS DependsOn(FROM Service TO Service);
CREATE REL TABLE IF NOT EXISTS Found(FROM Bug TO Layer);
