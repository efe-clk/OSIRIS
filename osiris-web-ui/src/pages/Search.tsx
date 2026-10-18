// Arama: tam metin (POST /search), semantik (POST /search/semantic), varlık (POST /search/entity).
// Sorgu URL'de tutulur (?mode=&type=&q=), böylece varlık bağlantıları buraya derin bağlanır.
import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { Api, ApiError, can, ENTITY_LABEL, ENTITY_TYPES, type ItemSummary } from "../api";
import { useAuth } from "../auth";
import { ItemDrawer } from "../itemDrawer";
import { useReportDraft } from "../reportDraft";
import { Empty, Notice, PageHeader, Segmented, Spinner, useUi } from "../ui";
import { downloadText, errMsg, fmtDate, hostOf, toCsv } from "../util";

type Mode = "fts" | "semantic" | "entity";
type Hit = ItemSummary & { similarity?: number };

function highlight(text: string, q: string) {
  const words = q.trim().split(/\s+/).filter((w) => w.length > 1).map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (!words.length) return text;
  const re = new RegExp(`(${words.join("|")})`, "gi");
  return text.split(re).map((part, i) => (i % 2 ? <mark key={i} className="rounded bg-osiris-accent/25 px-0.5 text-osiris-ink">{part}</mark> : part));
}

export default function Search() {
  const { role } = useAuth();
  const { toast } = useUi();
  const { addFinding } = useReportDraft();
  const [params, setParams] = useSearchParams();
  const [mode, setMode] = useState<Mode>((params.get("mode") as Mode) || "fts");
  const [etype, setEtype] = useState(params.get("type") || "email");
  const [q, setQ] = useState(params.get("q") || "");
  const [limit, setLimit] = useState(50);
  const [hits, setHits] = useState<Hit[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ text: string; kind: "error" | "warn" } | null>(null);
  const [openId, setOpenId] = useState<string | null>(null);
  const [ran, setRan] = useState<{ mode: Mode; q: string; type: string } | null>(null);

  const run = async (m = mode, query = q, t = etype) => {
    const s = query.trim();
    if (!s) return;
    setBusy(true);
    setError(null);
    try {
      const r: Hit[] = m === "fts" ? await Api.search(s, limit)
        : m === "semantic" ? await Api.searchSemantic(s, limit)
          : await Api.searchEntity(t, s, limit);
      setHits(r);
      setRan({ mode: m, q: s, type: t });
    } catch (e) {
      setHits(null);
      if (e instanceof ApiError && e.status === 503) {
        setError({ kind: "warn", text: "Semantik arama bu sunucuda kapalı. API servisinde OSIRIS_EMBEDDING_MODEL tanımlanmalı ve fastembed kurulu olmalı. Şimdilik tam metin aramayı kullanın." });
      } else setError({ kind: "error", text: errMsg(e) });
    } finally {
      setBusy(false);
    }
  };

  // URL'den gelen sorguyu bir kez çalıştır (ör. varlık bağlantısı)
  useEffect(() => {
    const pq = params.get("q");
    if (pq) {
      const m = (params.get("mode") as Mode) || "fts";
      const t = params.get("type") || "email";
      setMode(m); setQ(pq); setEtype(t);
      run(m, pq, t);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [params.toString()]);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const next: Record<string, string> = { mode, q: q.trim() };
    if (mode === "entity") next.type = etype;
    setParams(next);
  };

  const saveQuery = async () => {
    if (!ran) return;
    try {
      await Api.createSavedQuery({
        name: ran.mode === "entity" ? `${ENTITY_LABEL[ran.type as keyof typeof ENTITY_LABEL] ?? ran.type}: ${ran.q}` : ran.q,
        query_text: ran.q,
        query_type: ran.mode === "entity" ? "entity" : ran.mode,
        alert_enabled: true,
      });
      toast("Sorgu kaydedildi; yeni öğelerde eşleşirse uyarı üretir");
    } catch (e) {
      toast(errMsg(e), "error");
    }
  };

  const exportCsv = () => {
    if (!hits || !ran) return;
    downloadText(`osiris-arama.csv`, toCsv([
      ["id", "baslik", "url", "toplandi", "benzerlik", "ozet"],
      ...hits.map((h) => [h.id, h.title, h.url, h.collected_at ?? "", h.similarity ?? "", (h.cleaned_content ?? h.snippet ?? "").slice(0, 300)]),
    ]), "text/csv");
  };

  return (
    <div>
      <PageHeader title="Arama" lede="Toplanan tüm içerikte arayın. Tam metin kelime eşleşmesi, semantik arama anlam yakınlığı (çok dilli), varlık araması ise çıkarılmış e-posta, IP, alan adı gibi değerler üzerinden çalışır." />

      <form onSubmit={submit} className="panel space-y-4 p-5">
        <Segmented
          label="Arama türü"
          value={mode}
          onChange={(m) => { setMode(m); setError(null); }}
          options={[{ value: "fts", label: "Tam metin" }, { value: "semantic", label: "Semantik" }, { value: "entity", label: "Varlık" }]}
        />
        <div className="flex flex-col gap-2 sm:flex-row">
          {mode === "entity" && (
            <select id="s-type" aria-label="Varlık türü" className="field sm:w-44" value={etype} onChange={(e) => setEtype(e.target.value)}>
              {ENTITY_TYPES.map((t) => <option key={t} value={t}>{ENTITY_LABEL[t]}</option>)}
            </select>
          )}
          <input
            id="s-q"
            className="field h-11 text-base"
            maxLength={500}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            aria-label="Sorgu"
            placeholder={mode === "fts" ? "ransomware sızıntı" : mode === "semantic" ? "fidye yazılımı saldırısı hakkında haberler" : "ornek@alan.com"}
          />
          <select id="s-limit" aria-label="Sonuç sayısı" className="field sm:w-28" value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
            {[20, 50, 100].map((n) => <option key={n} value={n}>{n} sonuç</option>)}
          </select>
          <button className="btn-primary h-11 px-6" disabled={busy || !q.trim()}>Ara</button>
        </div>
        {mode === "entity" && <p className="text-xs text-osiris-faint">Değer kısmi eşleşir, büyük/küçük harf duyarsızdır.</p>}
      </form>

      {busy && <div className="mt-4"><Spinner label="Aranıyor" /></div>}
      {error && <div className="mt-4"><Notice kind={error.kind}>{error.text}</Notice></div>}

      {hits && ran && !busy && (
        <section className="mt-6">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
            <p className="text-sm text-osiris-muted">
              <span className="tnum text-osiris-ink">{hits.length}</span> sonuç · “{ran.q}”
              {hits.length === limit && <span className="text-osiris-faint"> · sınıra ulaşıldı, sorguyu daraltın</span>}
            </p>
            <div className="flex gap-2">
              <button className="btn-ghost py-1" onClick={exportCsv} disabled={!hits.length}>CSV indir</button>
              {can(role, "analyst") && <button className="btn-ghost py-1" onClick={saveQuery}>Uyarı olarak kaydet</button>}
            </div>
          </div>
          {hits.length === 0 ? (
            <Empty title="Sonuç yok">Farklı kelimeler deneyin{ran.mode === "fts" ? " ya da semantik aramaya geçin" : ""}.</Empty>
          ) : (
            <ul className="space-y-2">
              {hits.map((h) => {
                const text = (h.cleaned_content ?? h.snippet ?? "").slice(0, 320);
                return (
                  <li key={h.id} className="panel group">
                    <button onClick={() => setOpenId(h.id)} className="block w-full px-4 py-3 text-left hover:bg-osiris-raise">
                      <div className="flex items-baseline justify-between gap-3">
                        <span className="min-w-0 truncate font-medium">{h.title || h.url || "Başlıksız öğe"}</span>
                        {h.similarity !== undefined ? (
                          <span className="tnum shrink-0 text-xs text-osiris-accent">%{Math.round(h.similarity * 100)} benzer</span>
                        ) : h.collected_at ? (
                          <span className="shrink-0 text-xs text-osiris-faint">{fmtDate(h.collected_at)}</span>
                        ) : null}
                      </div>
                      <div className="truncate font-mono text-xs text-osiris-faint">{hostOf(h.url)}</div>
                      {text && <p className="mt-1 line-clamp-2 break-words text-sm text-osiris-muted">{ran.mode === "fts" ? highlight(text, ran.q) : text}</p>}
                    </button>
                    <div className="flex justify-end border-t border-osiris-line px-3 py-1.5 opacity-80 group-hover:opacity-100">
                      <button className="text-xs text-osiris-muted hover:text-osiris-ink" onClick={() => {
                        addFinding({ title: h.title || h.url || "Öğe", description: text }, h.url);
                        toast("Bulgu rapor taslağına eklendi");
                      }}>Rapora ekle</button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      )}
      <ItemDrawer id={openId} onClose={() => setOpenId(null)} onDeleted={(id) => setHits((hs) => hs?.filter((h) => h.id !== id) ?? null)} />
    </div>
  );
}
