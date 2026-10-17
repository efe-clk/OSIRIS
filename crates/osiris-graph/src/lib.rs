//! Varlık ilişki grafı (R6) — Python `osiris_graph.GraphEngine` ile aynı semantik.
//!
//! - networkx `Graph`: **yönsüz, çift başına tek kenar** (aynı çifte yeni `relation_type`
//!   öncekini yerinde ezer), öz döngü yok sayılır; düğüm, komşu ve kenar sırası networkx'teki
//!   ekleme sırasıdır (`/graph` gövdesi ve GraphML bu sırayla yazılır).
//! - Merkezilik: `nx.degree_centrality`. Topluluk: `nx.community.greedy_modularity_communities`
//!   (Clauset–Newman–Moore, ağırlıksız) — networkx 3.6.1 `_greedy_modularity_communities_generator`
//!   adım adım taşındı; `MappedQueue` yerine aynı tam sıralamalı (öncelik, eleman) kümesi
//!   kullanılır, yalnızca en küçük eleman gözlendiği için sonuç birebirdir.
//! - Sözleşme: docs/SOZLESME.md §9, fixture `fixtures/graph/cases.json`.
//!
//! Kilit yok: çağıran (API) grafı kendi kilidiyle sarar. DB kalıcılığı `osiris-db`'dedir.

use indexmap::IndexMap;
use osiris_hash::{py_strip, truncate_chars};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

pub const DEFAULT_MAX_NODES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GraphError {
    /// Python `ValueError`
    #[error("{0}")]
    Invalid(String),
    /// Python `OverflowError`
    #[error("Graf düğüm limiti aşıldı")]
    Overflow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeData {
    pub relation_type: String,
    pub weight: f64,
}

#[derive(Debug, Clone)]
pub struct Graph {
    pub max_nodes: usize,
    /// düğüm → nitelikler (ekleme sırası)
    nodes: IndexMap<String, IndexMap<String, String>>,
    /// düğüm → komşu → kenar dizini
    adj: IndexMap<String, IndexMap<String, usize>>,
    edges: Vec<EdgeData>,
}

impl Default for Graph {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_NODES)
    }
}

fn validate_id(entity_id: &str) -> Result<String, GraphError> {
    let eid = py_strip(entity_id);
    if eid.is_empty() {
        return Err(GraphError::Invalid("entity_id boş olamaz".into()));
    }
    if eid.chars().count() > 500 {
        return Err(GraphError::Invalid("entity_id çok uzun".into()));
    }
    Ok(eid.to_owned())
}

impl Graph {
    pub fn new(max_nodes: usize) -> Self {
        Self { max_nodes, nodes: IndexMap::new(), adj: IndexMap::new(), edges: Vec::new() }
    }

    pub fn number_of_nodes(&self) -> usize {
        self.nodes.len()
    }

    pub fn number_of_edges(&self) -> usize {
        self.edges.len()
    }

    pub fn has_node(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.adj.clear();
        self.edges.clear();
    }

    fn ensure_node(&mut self, id: &str) {
        if !self.nodes.contains_key(id) {
            self.nodes.insert(id.to_owned(), IndexMap::new());
            self.adj.insert(id.to_owned(), IndexMap::new());
        }
    }

    /// networkx `add_edge`: var olan kenarın nitelikleri yerinde güncellenir.
    fn add_edge(&mut self, u: &str, v: &str, data: EdgeData) {
        self.ensure_node(u);
        self.ensure_node(v);
        if let Some(&idx) = self.adj[u].get(v) {
            self.edges[idx] = data;
            return;
        }
        let idx = self.edges.len();
        self.edges.push(data);
        self.adj[u].insert(v.to_owned(), idx);
        self.adj[v].insert(u.to_owned(), idx);
    }

    /// `GraphEngine.add_relation`
    pub fn add_relation(
        &mut self,
        source: &str,
        target: &str,
        relation_type: &str,
        weight: f64,
    ) -> Result<(), GraphError> {
        let (source, target) = (validate_id(source)?, validate_id(target)?);
        if source == target {
            return Ok(());
        }
        if !weight.is_finite() {
            return Err(GraphError::Invalid("weight sonlu sayı olmalı".into()));
        }
        if self.nodes.len() >= self.max_nodes && !(self.has_node(&source) && self.has_node(&target))
        {
            return Err(GraphError::Overflow);
        }
        let data =
            EdgeData { relation_type: truncate_chars(relation_type, 100).to_owned(), weight };
        self.add_edge(&source, &target, data);
        Ok(())
    }

    /// `GraphEngine.add_entity` (nitelikler: ilk 20, anahtar 100 / değer 1000 kod noktası).
    pub fn add_entity(
        &mut self,
        entity_id: &str,
        attrs: &[(&str, &str)],
    ) -> Result<(), GraphError> {
        let eid = validate_id(entity_id)?;
        if self.nodes.len() >= self.max_nodes && !self.has_node(&eid) {
            return Err(GraphError::Overflow);
        }
        self.ensure_node(&eid);
        let node = &mut self.nodes[eid.as_str()];
        for (k, v) in attrs.iter().take(20) {
            node.insert(truncate_chars(k, 100).to_owned(), truncate_chars(v, 1000).to_owned());
        }
        Ok(())
    }

    /// networkx kenar görünümü: düğüm sırasıyla, her çift bir kez.
    pub fn edges(&self) -> Vec<(&str, &str, &EdgeData)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::with_capacity(self.edges.len());
        for (n, nbrs) in &self.adj {
            for (nbr, &idx) in nbrs {
                if !seen.contains(nbr.as_str()) {
                    out.push((n.as_str(), nbr.as_str(), &self.edges[idx]));
                }
            }
            seen.insert(n.as_str());
        }
        out
    }

    /// `GraphEngine.neighbors` (yoksa boş).
    pub fn neighbors(&self, id: &str) -> Vec<String> {
        self.adj.get(id).map(|n| n.keys().cloned().collect()).unwrap_or_default()
    }

    /// `nx.degree_centrality`: `derece / (n − 1)`; tek düğümde 1.0.
    pub fn centrality(&self) -> IndexMap<String, f64> {
        let n = self.nodes.len();
        if n <= 1 {
            return self.nodes.keys().map(|k| (k.clone(), 1.0)).collect();
        }
        let s = 1.0 / (n - 1) as f64;
        self.adj.iter().map(|(k, nbrs)| (k.clone(), nbrs.len() as f64 * s)).collect()
    }

    /// `GraphEngine.communities` — üyeler düğüm sırasıyla (topluluk/üye sırası sözleşme dışı).
    pub fn communities(&self) -> Vec<Vec<String>> {
        if self.nodes.is_empty() {
            return Vec::new();
        }
        let names: Vec<&str> = self.nodes.keys().map(String::as_str).collect();
        if self.edges.is_empty() {
            return names.iter().map(|n| vec![(*n).to_owned()]).collect();
        }
        let index: HashMap<&str, usize> = names.iter().enumerate().map(|(i, n)| (*n, i)).collect();
        let edges: Vec<(usize, usize)> =
            self.edges().into_iter().map(|(u, v, _)| (index[u], index[v])).collect();
        cnm(&names, &edges)
            .into_iter()
            .map(|c| c.into_iter().map(|i| names[i].to_owned()).collect())
            .collect()
    }

    /// `GraphEngine.to_json` (`/graph` gövdesi; DoS üst sınırlı).
    pub fn to_json(&self, limit_nodes: usize, limit_edges: usize) -> Value {
        let nodes: Vec<Value> = self
            .nodes
            .iter()
            .take(limit_nodes)
            .map(|(id, attrs)| {
                let mut m = Map::new();
                m.insert("id".into(), Value::String(id.clone()));
                for (k, v) in attrs {
                    m.insert(k.clone(), Value::String(v.clone()));
                }
                Value::Object(m)
            })
            .collect();
        let edges: Vec<Value> = self
            .edges()
            .into_iter()
            .take(limit_edges)
            .map(|(u, v, d)| {
                json!({"source": u, "target": v, "relation_type": d.relation_type,
                       "weight": d.weight})
            })
            .collect();
        json!({"nodes": nodes, "edges": edges})
    }

    /// `nx.write_graphml` çıktısı (bayt bayt; fixture `graphml`).
    pub fn graphml(&self) -> String {
        // anahtarlar oluşturulma sırasıyla d0, d1…; networkx her yeni anahtarı başa ekler
        let mut keys: Vec<(String, &'static str, &'static str)> = Vec::new();
        let mut key_of = |name: &str, scope: &'static str, ty: &'static str| -> String {
            let pos = keys.iter().position(|(n, s, t)| n == name && *s == scope && *t == ty);
            let idx = pos.unwrap_or_else(|| {
                keys.push((name.to_owned(), scope, ty));
                keys.len() - 1
            });
            format!("d{idx}")
        };
        let mut body = String::new();
        for (id, attrs) in &self.nodes {
            if attrs.is_empty() {
                let _ = writeln!(body, "    <node id=\"{}\" />", xml_attr(id));
                continue;
            }
            let _ = writeln!(body, "    <node id=\"{}\">", xml_attr(id));
            for (k, v) in attrs {
                let key = key_of(k, "node", "string");
                let _ = writeln!(body, "      <data key=\"{key}\">{}</data>", xml_text(v));
            }
            body.push_str("    </node>\n");
        }
        for (u, v, d) in self.edges() {
            let _ =
                writeln!(body, "    <edge source=\"{}\" target=\"{}\">", xml_attr(u), xml_attr(v));
            let k1 = key_of("relation_type", "edge", "string");
            let _ =
                writeln!(body, "      <data key=\"{k1}\">{}</data>", xml_text(&d.relation_type));
            let k2 = key_of("weight", "edge", "double");
            let _ = writeln!(
                body,
                "      <data key=\"{k2}\">{}</data>",
                osiris_hash::canonical::py_float_repr(d.weight)
            );
            body.push_str("    </edge>\n");
        }
        let mut out = String::from(
            "<?xml version='1.0' encoding='utf-8'?>\n<graphml xmlns=\"http://graphml.graphdrawing.org/xmlns\" \
             xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
             xsi:schemaLocation=\"http://graphml.graphdrawing.org/xmlns \
             http://graphml.graphdrawing.org/xmlns/1.0/graphml.xsd\">",
        );
        for (idx, (name, scope, ty)) in keys.iter().enumerate().rev() {
            let _ = write!(
                out,
                "\n  <key id=\"d{idx}\" for=\"{scope}\" attr.name=\"{}\" attr.type=\"{ty}\" />",
                xml_attr(name)
            );
        }
        if body.is_empty() {
            out.push_str("\n  <graph edgedefault=\"undirected\" />\n</graphml>\n");
        } else {
            out.push_str("\n  <graph edgedefault=\"undirected\">\n");
            out.push_str(&body);
            out.push_str("  </graph>\n</graphml>\n");
        }
        out
    }

    /// `nx.write_gexf` yapısı (GEXF 1.2draft). `meta` satırı tarihi ve üreticiyi içerir.
    pub fn gexf(&self) -> String {
        let mut attr_ids: Vec<(String, &'static str)> = Vec::new();
        let mut attr_id = |name: &str, class: &'static str| -> usize {
            attr_ids.iter().position(|(n, c)| n == name && *c == class).unwrap_or_else(|| {
                attr_ids.push((name.to_owned(), class));
                attr_ids.len() - 1
            })
        };
        let mut nodes = String::new();
        for (id, attrs) in &self.nodes {
            let id = xml_attr(id);
            if attrs.is_empty() {
                let _ = writeln!(nodes, "      <node id=\"{id}\" label=\"{id}\" />");
                continue;
            }
            let _ = writeln!(nodes, "      <node id=\"{id}\" label=\"{id}\">\n        <attvalues>");
            for (k, v) in attrs {
                let a = attr_id(k, "node");
                let _ =
                    writeln!(nodes, "          <attvalue for=\"{a}\" value=\"{}\" />", xml_attr(v));
            }
            nodes.push_str("        </attvalues>\n      </node>\n");
        }
        let mut edges = String::new();
        for (n, (u, v, d)) in self.edges().into_iter().enumerate() {
            let a = attr_id("relation_type", "edge");
            let _ = writeln!(
                edges,
                "      <edge source=\"{}\" target=\"{}\" id=\"{n}\" weight=\"{}\">\n        <attvalues>\n          \
                 <attvalue for=\"{a}\" value=\"{}\" />\n        </attvalues>\n      </edge>",
                xml_attr(u),
                xml_attr(v),
                osiris_hash::canonical::py_float_repr(d.weight),
                xml_attr(&d.relation_type)
            );
        }
        let mut out = format!(
            "<?xml version='1.0' encoding='utf-8'?>\n<gexf xmlns=\"http://www.gexf.net/1.2draft\" \
             xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
             xsi:schemaLocation=\"http://www.gexf.net/1.2draft http://www.gexf.net/1.2draft/gexf.xsd\" \
             version=\"1.2\">\n  <meta lastmodifieddate=\"{}\">\n    <creator>OSIRIS</creator>\n  </meta>\n  \
             <graph defaultedgetype=\"undirected\" mode=\"static\" name=\"\">\n",
            chrono::Utc::now().format("%Y-%m-%d")
        );
        for class in ["edge", "node"] {
            let defs: Vec<_> =
                attr_ids.iter().enumerate().filter(|(_, (_, c))| *c == class).collect();
            if defs.is_empty() {
                continue;
            }
            let _ = writeln!(out, "    <attributes mode=\"static\" class=\"{class}\">");
            for (i, (name, _)) in defs {
                let _ = writeln!(
                    out,
                    "      <attribute id=\"{i}\" title=\"{}\" type=\"string\" />",
                    xml_attr(name)
                );
            }
            out.push_str("    </attributes>\n");
        }
        if !nodes.is_empty() {
            let _ = write!(out, "    <nodes>\n{nodes}    </nodes>\n");
        }
        if !edges.is_empty() {
            let _ = write!(out, "    <edges>\n{edges}    </edges>\n");
        }
        out.push_str("  </graph>\n</gexf>\n");
        out
    }
}

/// ElementTree `_escape_cdata`
fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// ElementTree `_escape_attrib` (CPython 3.11)
fn xml_attr(s: &str) -> String {
    xml_text(s)
        .replace('"', "&quot;")
        .replace('\r', "&#13;")
        .replace('\n', "&#10;")
        .replace('\t', "&#09;")
}

// ------------------------------------------------------------------------------ CNM

/// `_HeapElement` önceliği: Python `==`/`<` (±0 eşit; NaN oluşmaz).
#[derive(Debug, Clone, Copy)]
struct Prio(f64);

impl PartialEq for Prio {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for Prio {}
impl PartialOrd for Prio {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Prio {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// (satır, sütun) — düğümlerin ad sırasındaki yerleri (Python demet karşılaştırması = dize sırası).
type Elem = (usize, usize);

/// networkx `MappedQueue`: öğe başına tek kayıt; `(öncelik, öğe)` tam sıralaması.
#[derive(Default)]
struct MappedQueue {
    set: BTreeSet<(Prio, Elem)>,
    prio: HashMap<Elem, f64>,
}

impl MappedQueue {
    fn len(&self) -> usize {
        self.set.len()
    }

    fn peek(&self) -> Option<(f64, Elem)> {
        self.set.first().map(|(p, e)| (p.0, *e))
    }

    fn push(&mut self, e: Elem, p: f64) -> bool {
        if self.prio.contains_key(&e) {
            return false;
        }
        self.prio.insert(e, p);
        self.set.insert((Prio(p), e));
        true
    }

    fn pop(&mut self) -> Option<(f64, Elem)> {
        let (p, e) = self.set.pop_first()?;
        self.prio.remove(&e);
        Some((p.0, e))
    }

    fn remove(&mut self, e: Elem) {
        if let Some(p) = self.prio.remove(&e) {
            self.set.remove(&(Prio(p), e));
        }
    }

    fn update(&mut self, old: Elem, new: Elem, p: f64) {
        self.remove(old);
        self.prio.insert(new, p);
        self.set.insert((Prio(p), new));
    }
}

/// `greedy_modularity_communities(G)` (weight=None, resolution=1, cutoff=1, best_n=None).
/// `names` düğüm sırası, `edges` networkx kenar sırası (dizinler `names`'e göre).
fn cnm(names: &[&str], edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let n = names.len();
    // dizinleri ad sırasına çevir: öğe karşılaştırması dize karşılaştırmasıdır
    let mut by_name: Vec<usize> = (0..n).collect();
    by_name.sort_by(|&a, &b| names[a].cmp(names[b]));
    let mut rank = vec![0; n];
    for (r, &i) in by_name.iter().enumerate() {
        rank[i] = r;
    }
    let unrank = by_name; // rank → düğüm sırası dizini

    let m = edges.len() as f64;
    let q0 = 1.0 / m;
    let mut degree = vec![0usize; n];
    for &(u, v) in edges {
        degree[rank[u]] += 1;
        degree[rank[v]] += 1;
    }
    // a = b (yönsüz): deg * q0 * 0.5
    let mut a: Vec<f64> = degree.iter().map(|&d| d as f64 * q0 * 0.5).collect();
    let mut dq: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); n];
    for &(u, v) in edges {
        let (u, v) = (rank[u], rank[v]);
        *dq[u].entry(v).or_insert(0.0) += 1.0;
        *dq[v].entry(u).or_insert(0.0) += 1.0;
    }
    for u in 0..n {
        let au = a[u];
        for (&v, wt) in dq[u].iter_mut() {
            *wt = q0 * *wt - (au * a[v] + au * a[v]);
        }
    }
    let mut heaps: Vec<MappedQueue> = (0..n).map(|_| MappedQueue::default()).collect();
    for u in 0..n {
        for (&v, &d) in &dq[u] {
            heaps[u].push((u, v), -d);
        }
    }
    let mut h = MappedQueue::default();
    for i in 0..n {
        if let Some((p, e)) = heaps[rank[i]].peek() {
            h.push(e, p);
        }
    }
    // topluluklar: sözlük sırası = G düğüm sırası; birleşmede v yerinde kalır, u silinir
    let mut communities: IndexMap<usize, Vec<usize>> =
        (0..n).map(|i| (rank[i], vec![rank[i]])).collect();

    while communities.len() > 1 {
        if h.len() <= 1 {
            break; // StopIteration: bağlı bileşenlere ulaşıldı
        }
        let Some((negdq, (u, v))) = h.pop() else { break };
        if -negdq < 0.0 {
            break; // modülerlik artık artmıyor
        }
        heaps[u].pop();
        if let Some((p, e)) = heaps[u].peek() {
            h.push(e, p);
        }
        if heaps[v].peek().map(|(_, e)| e) == Some((v, u)) {
            h.remove((v, u));
            heaps[v].remove((v, u));
            if let Some((p, e)) = heaps[v].peek() {
                h.push(e, p);
            }
        } else {
            heaps[v].remove((v, u));
        }
        let mut merged = communities.shift_remove(&u).unwrap_or_default();
        if let Some(cv) = communities.get_mut(&v) {
            cv.append(&mut merged);
        }

        let u_nbrs: BTreeSet<usize> = dq[u].keys().copied().collect();
        let v_nbrs: BTreeSet<usize> = dq[v].keys().copied().collect();
        let all_nbrs: BTreeSet<usize> =
            u_nbrs.union(&v_nbrs).copied().filter(|&w| w != u && w != v).collect();
        for &w in &all_nbrs {
            let dq_vw = if u_nbrs.contains(&w) && v_nbrs.contains(&w) {
                dq[v][&w] + dq[u][&w]
            } else if v_nbrs.contains(&w) {
                dq[v][&w] - (a[u] * a[w] + a[w] * a[u])
            } else {
                dq[u][&w] - (a[v] * a[w] + a[w] * a[v])
            };
            for (row, col) in [(v, w), (w, v)] {
                dq[row].insert(col, dq_vw);
                let old_max = heaps[row].peek();
                let d = (row, col);
                if v_nbrs.contains(&w) {
                    heaps[row].update(d, d, -dq_vw);
                } else {
                    heaps[row].push(d, -dq_vw);
                }
                match old_max {
                    None => {
                        h.push(d, -dq_vw);
                    }
                    Some((old_p, old_e)) => {
                        if let Some((row_p, row_e)) = heaps[row].peek()
                            && (old_e != row_e || old_p != row_p)
                        {
                            h.update(old_e, row_e, row_p);
                        }
                    }
                }
            }
        }
        let u_row: Vec<usize> = dq[u].keys().copied().collect();
        for w in u_row {
            dq[w].remove(&u);
            if w == v {
                continue;
            }
            for (row, col) in [(w, u), (u, w)] {
                let d_old = (row, col);
                if heaps[row].peek().map(|(_, e)| e) == Some(d_old) {
                    heaps[row].remove(d_old);
                    h.remove(d_old);
                    if let Some((p, e)) = heaps[row].peek() {
                        h.push(e, p);
                    }
                } else {
                    heaps[row].remove(d_old);
                }
            }
        }
        dq[u].clear();
        heaps[u] = MappedQueue::default();
        a[v] += a[u];
        a[u] = 0.0;
    }
    // büyükten küçüğe, eşitlerde sözlük sırası (Python sorted kararlıdır)
    let mut out: Vec<Vec<usize>> = communities
        .into_values()
        .map(|c| {
            let mut c: Vec<usize> = c.into_iter().map(|r| unrank[r]).collect();
            c.sort_unstable();
            c
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dogrulama_ve_sinir() {
        let mut g = Graph::new(2);
        assert_eq!(
            g.add_relation("  ", "b", "r", 1.0),
            Err(GraphError::Invalid("entity_id boş olamaz".into()))
        );
        assert!(g.add_relation(&"x".repeat(501), "b", "r", 1.0).is_err());
        assert!(g.add_relation("a", "b", "r", f64::NAN).is_err());
        g.add_relation(" a ", "b", "r", 1.0).unwrap();
        assert_eq!(g.add_relation("a", "c", "r", 1.0), Err(GraphError::Overflow));
        g.add_relation("a", "a", "r", 1.0).unwrap(); // öz döngü yok sayılır
        assert_eq!(g.number_of_edges(), 1);
        g.add_relation("b", "a", "yeni", 2.0).unwrap(); // ezer, sıra korunur
        assert_eq!(g.edges()[0].0, "a");
        assert_eq!(g.edges()[0].2.relation_type, "yeni");
        assert_eq!(g.neighbors("a"), vec!["b".to_owned()]);
        assert!(g.neighbors("yok").is_empty());
    }
}
