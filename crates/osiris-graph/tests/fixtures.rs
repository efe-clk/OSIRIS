//! `fixtures/graph` altın verileri: merkezilik (12 ondalık), topluluklar (sıralı), `/graph`
//! gövdesi ve GraphML (bayt bayt) Python `GraphEngine` + networkx 3.6.1 ile aynı.

use osiris_graph::Graph;
use serde_json::Value;
use std::path::PathBuf;

fn load(rel: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

/// Python `round(x, 12)` (fixture değerleri bu yuvarlamayla yazıldı)
fn round12(x: f64) -> f64 {
    format!("{x:.12}").parse().unwrap()
}

fn build(case: &Value) -> Graph {
    let mut g = Graph::default();
    let attrs = &case["node_attrs"];
    for node in case["nodes"].as_array().into_iter().flatten() {
        let id = node.as_str().unwrap();
        let kv: Vec<(&str, &str)> = attrs[id]
            .as_object()
            .map(|m| m.iter().map(|(k, v)| (k.as_str(), v.as_str().unwrap())).collect())
            .unwrap_or_default();
        g.add_entity(id, &kv).unwrap();
    }
    for e in case["edges"].as_array().unwrap() {
        let e = e.as_array().unwrap();
        let rel = e.get(2).and_then(Value::as_str).unwrap_or("related");
        let w = e.get(3).and_then(Value::as_f64).unwrap_or(1.0);
        g.add_relation(e[0].as_str().unwrap(), e[1].as_str().unwrap(), rel, w).unwrap();
    }
    g
}

#[test]
fn graph_fixtures() {
    let data = load("graph/cases.json");
    let cases = data["cases"].as_array().unwrap();
    assert!(cases.len() >= 12);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let exp = &case["expected"];
        let g = build(case);
        let cen = g.centrality();
        let want = exp["degree_centrality"].as_object().unwrap();
        assert_eq!(cen.len(), want.len(), "{name}: merkezilik boyutu");
        for (k, v) in want {
            assert_eq!(round12(cen[k.as_str()]), v.as_f64().unwrap(), "{name}: merkezilik {k}");
        }
        let mut comms: Vec<Vec<String>> = g
            .communities()
            .into_iter()
            .map(|mut c| {
                c.sort();
                c
            })
            .collect();
        comms.sort();
        let want: Vec<Vec<String>> = serde_json::from_value(exp["communities"].clone()).unwrap();
        assert_eq!(comms, want, "{name}: topluluklar");
        assert_eq!(g.to_json(5000, 20000), exp["to_json"], "{name}: to_json");
        assert_eq!(g.graphml(), exp["graphml"].as_str().unwrap(), "{name}: graphml");
    }
}

#[test]
fn gexf_yapisi() {
    let mut g = Graph::default();
    g.add_entity("a", &[("kind", "ip<&>")]).unwrap();
    g.add_relation("a", "b", "owns", 2.5).unwrap();
    let x = g.gexf();
    assert!(x.contains("<attvalue for=\"0\" value=\"ip&lt;&amp;&gt;\" />"), "{x}");
    assert!(x.contains("<edge source=\"a\" target=\"b\" id=\"0\" weight=\"2.5\">"), "{x}");
    assert!(x.contains("<attribute id=\"1\" title=\"relation_type\" type=\"string\" />"), "{x}");
}
