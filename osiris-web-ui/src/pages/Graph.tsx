// İlişki grafı: GET /graph (düğüm/kenar), GET /graph/analysis (merkezilik, CNM toplulukları,
// komşular), POST /graph/relation (kalıcı kenar), GET /graph/export (GraphML/GEXF).
// Yerleşim bağımlılıksız bir kuvvet simülasyonudur (canvas); kaydır, yakınlaştır, düğüme tıkla.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { Api, can, type GraphAnalysis, type GraphData } from "../api";
import { useAuth } from "../auth";
import { Empty, Label, Loading, Notice, PageHeader, Pill, useLoad, useUi } from "../ui";
import { downloadText, errMsg } from "../util";

const PALETTE = ["#22d3ee", "#a78bfa", "#4ade80", "#fbbf24", "#f472b6", "#fb923c", "#60a5fa", "#a3e635", "#e879f9", "#2dd4bf"];
const MAX_DRAW = 1500;

interface SimNode { id: string; x: number; y: number; vx: number; vy: number; deg: number; comm: number }
interface SimEdge { s: number; t: number; rel: string }

function useSimulation(data: GraphData | null, comms: Map<string, number>) {
  return useMemo(() => {
    if (!data) return { nodes: [] as SimNode[], edges: [] as SimEdge[], truncated: false };
    const truncated = data.nodes.length > MAX_DRAW;
    const ids = data.nodes.slice(0, MAX_DRAW).map((n) => n.id);
    const index = new Map(ids.map((id, i) => [id, i]));
    const nodes: SimNode[] = ids.map((id, i) => {
      const a = i * 2.399963; // altın açı: başlangıçta üst üste binmesin
      const r = 12 * Math.sqrt(i + 1);
      return { id, x: Math.cos(a) * r, y: Math.sin(a) * r, vx: 0, vy: 0, deg: 0, comm: comms.get(id) ?? -1 };
    });
    const edges: SimEdge[] = [];
    for (const e of data.edges) {
      const s = index.get(e.source), t = index.get(e.target);
      if (s === undefined || t === undefined || s === t) continue;
      edges.push({ s, t, rel: e.relation_type ?? "" });
      nodes[s].deg++; nodes[t].deg++;
    }
    return { nodes, edges, truncated };
  }, [data, comms]);
}

function GraphCanvas({ nodes, edges, selected, onSelect, highlight }: {
  nodes: SimNode[]; edges: SimEdge[]; selected: string | null; onSelect: (id: string | null) => void; highlight: Set<string>;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const view = useRef({ x: 0, y: 0, k: 1 });
  const alpha = useRef(1);
  const drag = useRef<{ mode: "pan" | "node"; i: number; sx: number; sy: number; moved: boolean } | null>(null);
  const frame = useRef(0);
  const fitted = useRef(false);
  const selRef = useRef(selected);
  const hlRef = useRef(highlight);
  selRef.current = selected;
  hlRef.current = highlight;

  const toWorld = (cx: number, cy: number) => {
    const c = canvas.current!;
    const r = c.getBoundingClientRect();
    const v = view.current;
    return { x: (cx - r.left - r.width / 2 - v.x) / v.k, y: (cy - r.top - r.height / 2 - v.y) / v.k };
  };
  const hit = (wx: number, wy: number) => {
    let best = -1, bd = Infinity;
    nodes.forEach((n, i) => {
      const rad = 4 + Math.min(10, Math.sqrt(n.deg) * 2);
      const d = (n.x - wx) ** 2 + (n.y - wy) ** 2;
      if (d < (rad + 4 / view.current.k) ** 2 && d < bd) { bd = d; best = i; }
    });
    return best;
  };

  const fit = useCallback(() => {
    const c = canvas.current;
    if (!c || nodes.length === 0) return;
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const n of nodes) { x0 = Math.min(x0, n.x); y0 = Math.min(y0, n.y); x1 = Math.max(x1, n.x); y1 = Math.max(y1, n.y); }
    const pad = 70;
    const k = Math.max(0.15, Math.min(2.5, (c.clientWidth - pad * 2) / Math.max(1, x1 - x0), (c.clientHeight - pad * 2) / Math.max(1, y1 - y0)));
    view.current = { k, x: -((x0 + x1) / 2) * k, y: -((y0 + y1) / 2) * k };
  }, [nodes]);

  useEffect(() => {
    alpha.current = 1;
    fitted.current = false;
    const c = canvas.current;
    if (!c) return;
    const ctx = c.getContext("2d")!;
    const reduce = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    const tick = () => {
      // Kuvvetler: itme (O(n²), n ≤ 1500), yay, merkeze çekim
      if (alpha.current > 0.005) {
        const a = alpha.current;
        const n = nodes.length;
        const steps = reduce ? 1 : n > 600 ? 1 : 2;
        for (let s = 0; s < steps; s++) {
          for (let i = 0; i < n; i++) {
            const p = nodes[i];
            for (let j = i + 1; j < n; j++) {
              const q = nodes[j];
              let dx = p.x - q.x, dy = p.y - q.y;
              let d2 = dx * dx + dy * dy;
              if (d2 > 160000) continue;
              if (d2 < 0.01) { dx = Math.random() - 0.5; dy = Math.random() - 0.5; d2 = 0.25; }
              const f = (1400 * a) / d2;
              p.vx += dx * f; p.vy += dy * f; q.vx -= dx * f; q.vy -= dy * f;
            }
          }
          for (const e of edges) {
            const p = nodes[e.s], q = nodes[e.t];
            const dx = q.x - p.x, dy = q.y - p.y;
            const d = Math.sqrt(dx * dx + dy * dy) || 1;
            const f = ((d - 64) / d) * 0.06 * a;
            p.vx += dx * f; p.vy += dy * f; q.vx -= dx * f; q.vy -= dy * f;
          }
          for (const p of nodes) {
            if (drag.current?.mode === "node" && nodes[drag.current.i] === p) { p.vx = p.vy = 0; continue; }
            p.vx -= p.x * 0.004 * a; p.vy -= p.y * 0.004 * a;
            p.vx *= 0.6; p.vy *= 0.6;
            p.x += p.vx; p.y += p.vy;
          }
          alpha.current *= reduce ? 0.9 : 0.985;
        }
        // Yerleşim oturunca bir kez görünüme sığdır (kullanıcı kaydırmadıysa)
        if (!fitted.current && alpha.current < 0.12) { fitted.current = true; if (!selRef.current) fit(); }
      }
      // Çizim
      const dpr = window.devicePixelRatio || 1;
      const w = c.clientWidth, h = c.clientHeight;
      if (c.width !== w * dpr || c.height !== h * dpr) { c.width = w * dpr; c.height = h * dpr; }
      const v = view.current;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      ctx.translate(w / 2 + v.x, h / 2 + v.y);
      ctx.scale(v.k, v.k);
      const sel = selRef.current, hl = hlRef.current;
      const focus = sel !== null;
      ctx.lineWidth = 1 / v.k;
      for (const e of edges) {
        const p = nodes[e.s], q = nodes[e.t];
        const on = focus && (p.id === sel || q.id === sel);
        ctx.strokeStyle = on ? "rgba(34,211,238,0.85)" : focus ? "rgba(93,111,128,0.15)" : "rgba(93,111,128,0.45)";
        ctx.beginPath(); ctx.moveTo(p.x, p.y); ctx.lineTo(q.x, q.y); ctx.stroke();
        if (on && e.rel && v.k > 0.7) {
          ctx.fillStyle = "#8a9bab";
          ctx.font = `${10 / v.k}px "IBM Plex Mono", monospace`;
          ctx.fillText(e.rel, (p.x + q.x) / 2 + 3 / v.k, (p.y + q.y) / 2 - 3 / v.k);
        }
      }
      for (const p of nodes) {
        const rad = 4 + Math.min(10, Math.sqrt(p.deg) * 2);
        const isSel = p.id === sel;
        const dim = focus && !isSel && !hl.has(p.id);
        ctx.globalAlpha = dim ? 0.25 : 1;
        ctx.fillStyle = p.comm >= 0 ? PALETTE[p.comm % PALETTE.length] : "#8a9bab";
        ctx.beginPath(); ctx.arc(p.x, p.y, rad, 0, Math.PI * 2); ctx.fill();
        if (isSel) { ctx.strokeStyle = "#d6e1ea"; ctx.lineWidth = 2 / v.k; ctx.stroke(); ctx.lineWidth = 1 / v.k; }
        if ((isSel || hl.has(p.id) || p.deg >= 4 || v.k > 1.6 || nodes.length <= 60) && !dim) {
          ctx.fillStyle = "#d6e1ea";
          ctx.font = `${(isSel ? 12 : 11) / v.k}px "IBM Plex Sans", sans-serif`;
          ctx.fillText(p.id.length > 32 ? p.id.slice(0, 31) + "…" : p.id, p.x + rad + 3 / v.k, p.y + 4 / v.k);
        }
        ctx.globalAlpha = 1;
      }
      frame.current = requestAnimationFrame(tick);
    };
    frame.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame.current);
  }, [nodes, edges, fit]);

  // Seçili düğüme odaklan
  useEffect(() => {
    const n = nodes.find((x) => x.id === selected);
    if (n) { view.current.x = -n.x * view.current.k; view.current.y = -n.y * view.current.k; }
  }, [selected, nodes]);

  const onDown = (e: React.PointerEvent) => {
    const w = toWorld(e.clientX, e.clientY);
    const i = hit(w.x, w.y);
    drag.current = { mode: i >= 0 ? "node" : "pan", i, sx: e.clientX, sy: e.clientY, moved: false };
    fitted.current = true; // kullanıcı etkileşti: otomatik sığdırma yapma
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };
  const onMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    if (Math.abs(e.clientX - d.sx) + Math.abs(e.clientY - d.sy) > 3) d.moved = true;
    if (d.mode === "pan") {
      view.current.x += e.movementX; view.current.y += e.movementY;
    } else {
      const w = toWorld(e.clientX, e.clientY);
      nodes[d.i].x = w.x; nodes[d.i].y = w.y;
      alpha.current = Math.max(alpha.current, 0.3);
    }
  };
  const onUp = () => {
    const d = drag.current;
    drag.current = null;
    if (d && !d.moved) onSelect(d.i >= 0 ? nodes[d.i].id : null);
  };
  const onWheel = (e: React.WheelEvent) => {
    fitted.current = true;
    const v = view.current;
    const k = Math.min(6, Math.max(0.15, v.k * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
    const r = canvas.current!.getBoundingClientRect();
    const mx = e.clientX - r.left - r.width / 2, my = e.clientY - r.top - r.height / 2;
    v.x = mx - ((mx - v.x) * k) / v.k; v.y = my - ((my - v.y) * k) / v.k; v.k = k;
  };

  return (
    <div className="relative">
      <canvas
        ref={canvas}
        className="h-[62vh] min-h-[420px] w-full touch-none cursor-grab rounded-lg border border-osiris-line bg-osiris-sink active:cursor-grabbing"
        onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onPointerCancel={() => (drag.current = null)} onWheel={onWheel}
        role="img" aria-label={`İlişki grafı: ${nodes.length} düğüm, ${edges.length} kenar`}
      />
      <div className="absolute right-3 top-3 flex gap-1">
        <button className="btn-ghost bg-osiris-bg/80 px-2 py-1 text-xs" onClick={() => { view.current.k = Math.min(6, view.current.k * 1.3); }}>+</button>
        <button className="btn-ghost bg-osiris-bg/80 px-2 py-1 text-xs" onClick={() => { view.current.k = Math.max(0.15, view.current.k / 1.3); }}>−</button>
        <button className="btn-ghost bg-osiris-bg/80 px-2 py-1 text-xs" onClick={fit}>Sığdır</button>
      </div>
      <p className="mt-1.5 text-xs text-osiris-faint">Sürükleyerek kaydırın, tekerlekle yakınlaştırın, düğümü sürükleyip bırakın. Renkler toplulukları gösterir.</p>
    </div>
  );
}

export default function Graph() {
  const { role } = useAuth();
  const { toast } = useUi();
  const [params, setParams] = useSearchParams();
  const graph = useLoad(() => Api.graph());
  const analysis = useLoad(() => Api.graphAnalysis(30));
  const [selected, setSelected] = useState<string | null>(params.get("node"));
  const [neighbors, setNeighbors] = useState<string[]>([]);
  const [find, setFind] = useState("");
  const [rel, setRel] = useState({ source: "", target: "", relation_type: "related" });
  const [adding, setAdding] = useState(false);

  const comms = useMemo(() => {
    const m = new Map<string, number>();
    (analysis.data as GraphAnalysis | null)?.communities?.forEach((c, i) => c.members.forEach((id) => m.set(id, i)));
    return m;
  }, [analysis.data]);
  const sim = useSimulation(graph.data, comms);

  const select = useCallback((id: string | null) => {
    setSelected(id);
    setParams(id ? { node: id } : {}, { replace: true });
  }, [setParams]);

  useEffect(() => {
    if (!selected) { setNeighbors([]); return; }
    Api.graphAnalysis(1, selected).then((a) => setNeighbors(a.neighbors ?? [])).catch(() => setNeighbors([]));
  }, [selected]);

  const reload = () => { graph.reload(); analysis.reload(); };

  const addRelation = async (e: React.FormEvent) => {
    e.preventDefault();
    setAdding(true);
    try {
      await Api.addRelation(rel.source.trim(), rel.target.trim(), rel.relation_type.trim() || "related");
      toast("İlişki eklendi ve kaydedildi");
      setRel({ source: "", target: "", relation_type: rel.relation_type });
      reload();
    } catch (err) {
      toast(errMsg(err), "error");
    } finally {
      setAdding(false);
    }
  };

  const exportAs = async (fmt: "graphml" | "gexf") => {
    try {
      downloadText(`osiris-graf.${fmt}`, await Api.graphExport(fmt), "application/xml");
    } catch (err) {
      toast(errMsg(err), "error");
    }
  };

  const a = analysis.data;
  const matches = find.trim()
    ? sim.nodes.filter((n) => n.id.toLowerCase().includes(find.trim().toLowerCase())).slice(0, 8)
    : [];

  return (
    <div>
      <PageHeader
        title="İlişki grafı"
        lede="Varlıklar arasındaki kalıcı ilişkiler. Kenarlar elle, kullanıcı adı taramasından ya da otomasyonla eklenir. Merkezilik en çok bağlantısı olan düğümleri, topluluklar birbirine sıkı bağlı kümeleri gösterir."
        actions={
          <>
            <button className="btn-ghost" onClick={reload}>Yenile</button>
            <button className="btn-ghost" onClick={() => exportAs("graphml")}>GraphML indir</button>
            <button className="btn-ghost" onClick={() => exportAs("gexf")}>GEXF indir</button>
          </>
        }
      />
      {graph.error && <Notice>{graph.error}</Notice>}
      {graph.loading && !graph.data ? <Loading /> : sim.nodes.length === 0 ? (
        <Empty title="Graf boş">
          Kullanıcı adı taramasında “Grafa bağla”yı kullanın ya da aşağıdan elle bir ilişki ekleyin (ör. alan adı → IP, “resolves”).
        </Empty>
      ) : (
        <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
          <div className="min-w-0">
            <div className="relative mb-3 max-w-md">
              <input id="g-find" className="field" placeholder="Düğüm bul" aria-label="Düğüm bul" value={find} onChange={(e) => setFind(e.target.value)} />
              {matches.length > 0 && (
                <ul className="panel absolute z-10 mt-1 w-full py-1 shadow-xl">
                  {matches.map((m) => (
                    <li key={m.id}><button className="block w-full truncate px-3 py-1.5 text-left text-sm hover:bg-osiris-raise" onClick={() => { select(m.id); setFind(""); }}>{m.id}</button></li>
                  ))}
                </ul>
              )}
            </div>
            <GraphCanvas nodes={sim.nodes} edges={sim.edges} selected={selected} onSelect={select} highlight={new Set(neighbors)} />
            {sim.truncated && <Notice kind="warn">Graf {graph.data?.nodes.length} düğüm içeriyor; ilk {MAX_DRAW} düğüm çiziliyor. Tamamı için GraphML indirip Gephi'de açın.</Notice>}
          </div>

          <aside className="space-y-5">
            <div className="panel tnum grid grid-cols-3 divide-x divide-osiris-line text-center">
              <div className="p-3"><div className="text-lg font-semibold">{a?.nodes ?? sim.nodes.length}</div><div className="text-xs text-osiris-muted">düğüm</div></div>
              <div className="p-3"><div className="text-lg font-semibold">{a?.edges ?? sim.edges.length}</div><div className="text-xs text-osiris-muted">kenar</div></div>
              <div className="p-3"><div className="text-lg font-semibold">{a?.communities?.length ?? "—"}</div><div className="text-xs text-osiris-muted">topluluk</div></div>
            </div>

            {selected && (
              <div className="panel p-4">
                <div className="mb-2 flex items-start justify-between gap-2">
                  <div className="min-w-0">
                    <div className="text-xs text-osiris-muted">Seçili düğüm</div>
                    <div className="break-words font-mono text-sm">{selected}</div>
                  </div>
                  <button className="text-xs text-osiris-muted hover:text-osiris-ink" onClick={() => select(null)}>Bırak</button>
                </div>
                <p className="mb-2 text-xs text-osiris-faint">{neighbors.length} komşu</p>
                <ul className="max-h-48 space-y-1 overflow-y-auto text-sm">
                  {neighbors.map((n) => (
                    <li key={n}><button className="block max-w-full truncate text-left hover:text-osiris-accent" onClick={() => select(n)}>{n}</button></li>
                  ))}
                </ul>
                <div className="mt-3 flex flex-wrap gap-3 border-t border-osiris-line pt-3 text-xs">
                  <Link className="text-osiris-accent hover:underline" to={`/search?q=${encodeURIComponent(selected)}`}>İçerikte ara</Link>
                  {can(role, "analyst") && <button className="text-osiris-muted hover:text-osiris-ink" onClick={() => setRel((r) => ({ ...r, source: selected }))}>Bu düğümden ilişki ekle</button>}
                </div>
              </div>
            )}

            <div>
              <h2 className="mb-2 text-sm font-semibold">En merkezi düğümler</h2>
              {analysis.error ? <p className="text-sm text-osiris-bad">{analysis.error}</p> : (
                <ol className="panel divide-y divide-osiris-line text-sm">
                  {(a?.centrality ?? []).slice(0, 12).map((c, i) => (
                    <li key={c.id}>
                      <button onClick={() => select(c.id)} className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-osiris-raise">
                        <span className="tnum w-5 text-xs text-osiris-faint">{i + 1}</span>
                        <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: comms.has(c.id) ? PALETTE[comms.get(c.id)! % PALETTE.length] : "#8a9bab" }} />
                        <span className="min-w-0 flex-1 truncate">{c.id}</span>
                        <span className="tnum text-xs text-osiris-muted">{c.degree}</span>
                      </button>
                    </li>
                  ))}
                </ol>
              )}
            </div>

            {a?.communities && a.communities.length > 1 && (
              <div>
                <h2 className="mb-2 text-sm font-semibold">Topluluklar</h2>
                <ul className="space-y-1.5 text-sm">
                  {a.communities.slice(0, 8).map((c, i) => (
                    <li key={i} className="flex items-start gap-2">
                      <span className="mt-1.5 h-2 w-2 shrink-0 rounded-full" style={{ background: PALETTE[i % PALETTE.length] }} />
                      <span className="min-w-0 text-osiris-muted"><span className="tnum text-osiris-ink">{c.size}</span> · {c.members.slice(0, 4).join(", ")}{c.size > 4 ? "…" : ""}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {a && a.communities === null && <Pill tone="warn">Topluluk tespiti 5000 düğüm üstünde kapalı</Pill>}
          </aside>
        </div>
      )}

      {can(role, "analyst") && (
        <form onSubmit={addRelation} className="panel mt-8 p-5">
          <h2 className="mb-3 font-semibold">İlişki ekle</h2>
          <div className="grid gap-3 sm:grid-cols-[1fr_1fr_12rem_auto] sm:items-end">
            <Label label="Kaynak" htmlFor="r-src"><input id="r-src" className="field font-mono" maxLength={500} value={rel.source} onChange={(e) => setRel({ ...rel, source: e.target.value })} placeholder="ornek.com" /></Label>
            <Label label="Hedef" htmlFor="r-tgt"><input id="r-tgt" className="field font-mono" maxLength={500} value={rel.target} onChange={(e) => setRel({ ...rel, target: e.target.value })} placeholder="203.0.113.7" /></Label>
            <Label label="İlişki türü" htmlFor="r-type">
              <input id="r-type" className="field" list="rel-types" maxLength={100} value={rel.relation_type} onChange={(e) => setRel({ ...rel, relation_type: e.target.value })} />
              <datalist id="rel-types">
                {["related", "resolves", "has_account", "owns", "member_of", "mentions", "same_as", "located_in"].map((r) => <option key={r} value={r} />)}
              </datalist>
            </Label>
            <button className="btn-primary" disabled={adding || !rel.source.trim() || !rel.target.trim()}>Ekle</button>
          </div>
          <p className="mt-2 text-xs text-osiris-faint">Graf yönsüzdür; aynı çifte yeni tür eklemek bellekteki kenarı günceller.</p>
        </form>
      )}
    </div>
  );
}
