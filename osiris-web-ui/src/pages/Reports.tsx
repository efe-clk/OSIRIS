// Raporlar: ekranlardan “Rapora ekle” ile biriken taslak → POST /reports/generate
// (Markdown, HTML, JSON, STIX 2.1). Taslak bu tarayıcıda tutulur.
import { useState } from "react";
import { Link } from "react-router-dom";
import { Api, can, ENTITY_LABEL, ENTITY_TYPES, type ReportFormat, type ReportOut } from "../api";
import { useAuth } from "../auth";
import { useReportDraft } from "../reportDraft";
import { Empty, EntityTypeTag, Label, Notice, PageHeader, Segmented, useUi } from "../ui";
import { copyText, downloadText, errMsg, slug, toCsv } from "../util";

const FORMATS: { value: ReportFormat; label: string; hint: string }[] = [
  { value: "markdown", label: "Markdown", hint: "Okunabilir metin; wiki, Git ya da not uygulaması için." },
  { value: "html", label: "HTML", hint: "Tarayıcıda açılır, yazdırılıp PDF'e çevrilebilir. İçerik kaçışlıdır." },
  { value: "json", label: "JSON", hint: "Makine okunur tam döküm (bulgular, kaynaklar, göstergeler)." },
  { value: "stix", label: "STIX 2.1", hint: "Tehdit istihbaratı paylaşımı. Yalnızca IP, alan adı, e-posta, kripto adresi, hash ve CVE göstergeleri girer." },
];

const STIX_TYPES = new Set(["ip", "domain", "email", "crypto_address", "hash", "cve"]);

export default function Reports() {
  const { role } = useAuth();
  const { toast, confirm } = useUi();
  const { draft, setDraft, clear } = useReportDraft();
  const [format, setFormat] = useState<ReportFormat>("markdown");
  const [out, setOut] = useState<ReportOut | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newSource, setNewSource] = useState("");
  const [ind, setInd] = useState({ type: "ip", value: "" });
  const canGen = can(role, "analyst");

  const set = (patch: Partial<typeof draft>) => setDraft({ ...draft, ...patch });
  const setFinding = (i: number, patch: Partial<{ title: string; description: string }>) =>
    set({ findings: draft.findings.map((f, k) => (k === i ? { ...f, ...patch } : f)) });
  const move = (i: number, d: -1 | 1) => {
    const j = i + d;
    if (j < 0 || j >= draft.findings.length) return;
    const f = [...draft.findings];
    [f[i], f[j]] = [f[j], f[i]];
    set({ findings: f });
  };

  const generate = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await Api.reportGenerate({
        title: draft.title.trim() || "OSIRIS raporu", scope: draft.scope, summary: draft.summary,
        findings: draft.findings, sources: draft.sources, entities: draft.entities, format,
      });
      setOut(r);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const stixCount = draft.entities.filter((e) => STIX_TYPES.has(e.type)).length;

  return (
    <div>
      <PageHeader
        title="Raporlar"
        lede="Soruşturma boyunca öğe, varlık ve tarama sonuçlarından “Rapora ekle” ile bulgu toplayın; burada düzenleyip Markdown, HTML, JSON ya da STIX 2.1 olarak üretin. Taslak yalnızca bu tarayıcıda saklanır."
        actions={
          <>
            <button className="btn-ghost" disabled={!draft.findings.length} onClick={() => downloadText(`${slug(draft.title)}-bulgular.csv`,
              toCsv([["baslik", "aciklama"], ...draft.findings.map((f) => [f.title, f.description])]), "text/csv")}>Bulguları CSV indir</button>
            <button className="btn-danger" onClick={async () => {
              if (await confirm({ title: "Taslak temizlensin mi?", body: "Başlık, özet, tüm bulgular, kaynaklar ve göstergeler silinir.", confirmLabel: "Temizle", danger: true })) { clear(); setOut(null); }
            }}>Taslağı temizle</button>
          </>
        }
      />

      <div className="grid gap-8 xl:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
        <div className="min-w-0 space-y-6">
          <div className="panel space-y-4 p-5">
            <Label label="Başlık" htmlFor="r-title"><input id="r-title" className="field text-base" maxLength={500} value={draft.title} onChange={(e) => set({ title: e.target.value })} placeholder="Ör. ornekkurum.com.tr dış saldırı yüzeyi" /></Label>
            <Label label="Kapsam" htmlFor="r-scope"><input id="r-scope" className="field" maxLength={1000} value={draft.scope} onChange={(e) => set({ scope: e.target.value })} placeholder="Zaman aralığı, hedefler, kullanılan kaynaklar" /></Label>
            <Label label="Yönetici özeti" htmlFor="r-sum"><textarea id="r-sum" className="field min-h-[6rem]" maxLength={20000} value={draft.summary} onChange={(e) => set({ summary: e.target.value })} /></Label>
          </div>

          <section>
            <div className="mb-2 flex items-center justify-between">
              <h2 className="font-semibold">Bulgular <span className="tnum text-osiris-faint">{draft.findings.length}</span></h2>
              <button className="btn-ghost py-1" onClick={() => set({ findings: [...draft.findings, { title: "", description: "" }] })}>Bulgu ekle</button>
            </div>
            {draft.findings.length === 0 ? (
              <Empty title="Henüz bulgu yok">
                <Link to="/search" className="text-osiris-accent hover:underline">Arama</Link>, <Link to="/entities" className="text-osiris-accent hover:underline">Varlıklar</Link> ve <Link to="/username" className="text-osiris-accent hover:underline">Kullanıcı adı tarama</Link> ekranlarındaki “Rapora ekle” düğmelerini kullanın ya da elle ekleyin.
              </Empty>
            ) : (
              <ol className="space-y-3">
                {draft.findings.map((f, i) => (
                  <li key={i} className="panel p-4">
                    <div className="mb-2 flex items-center gap-2">
                      <span className="tnum w-6 text-sm text-osiris-faint">{i + 1}.</span>
                      <input aria-label={`Bulgu ${i + 1} başlığı`} className="field font-medium" maxLength={500} value={f.title} onChange={(e) => setFinding(i, { title: e.target.value })} placeholder="Bulgu başlığı" />
                      <div className="flex shrink-0 gap-1">
                        <button className="btn-ghost px-2 py-1 text-xs" disabled={i === 0} onClick={() => move(i, -1)} aria-label="Yukarı taşı">↑</button>
                        <button className="btn-ghost px-2 py-1 text-xs" disabled={i === draft.findings.length - 1} onClick={() => move(i, 1)} aria-label="Aşağı taşı">↓</button>
                        <button className="btn-ghost px-2 py-1 text-xs hover:text-osiris-bad" onClick={() => set({ findings: draft.findings.filter((_, k) => k !== i) })}>Sil</button>
                      </div>
                    </div>
                    <textarea aria-label={`Bulgu ${i + 1} açıklaması`} className="field min-h-[4.5rem] text-[13px]" maxLength={5000} value={f.description} onChange={(e) => setFinding(i, { description: e.target.value })} />
                  </li>
                ))}
              </ol>
            )}
          </section>

          <div className="grid gap-6 md:grid-cols-2 xl:grid-cols-1 2xl:grid-cols-2">
            <section>
              <h2 className="mb-2 font-semibold">Kaynaklar <span className="tnum text-osiris-faint">{draft.sources.length}</span></h2>
              <form className="mb-2 flex gap-2" onSubmit={(e) => { e.preventDefault(); if (newSource.trim()) { set({ sources: [...draft.sources, newSource.trim()] }); setNewSource(""); } }}>
                <input id="r-src" className="field" placeholder="https://… ya da kaynak adı" aria-label="Kaynak ekle" value={newSource} onChange={(e) => setNewSource(e.target.value)} />
                <button className="btn-ghost" disabled={!newSource.trim()}>Ekle</button>
              </form>
              <ul className="max-h-56 space-y-1 overflow-y-auto text-sm">
                {draft.sources.map((s, i) => (
                  <li key={i} className="flex items-center justify-between gap-2 rounded border border-osiris-line px-2 py-1">
                    <span className="min-w-0 truncate font-mono text-xs">{s}</span>
                    <button className="text-xs text-osiris-muted hover:text-osiris-bad" onClick={() => set({ sources: draft.sources.filter((_, k) => k !== i) })}>Çıkar</button>
                  </li>
                ))}
              </ul>
            </section>
            <section>
              <h2 className="mb-2 font-semibold">Göstergeler <span className="tnum text-osiris-faint">{draft.entities.length}</span></h2>
              <form className="mb-2 flex gap-2" onSubmit={(e) => {
                e.preventDefault();
                if (!ind.value.trim()) return;
                set({ entities: [...draft.entities, { type: ind.type, value: ind.value.trim() }] });
                setInd({ ...ind, value: "" });
              }}>
                <select aria-label="Gösterge türü" className="field w-32" value={ind.type} onChange={(e) => setInd({ ...ind, type: e.target.value })}>
                  {ENTITY_TYPES.map((t) => <option key={t} value={t}>{ENTITY_LABEL[t]}</option>)}
                </select>
                <input id="r-ind" className="field font-mono" placeholder="203.0.113.7" aria-label="Gösterge değeri" value={ind.value} onChange={(e) => setInd({ ...ind, value: e.target.value })} />
                <button className="btn-ghost" disabled={!ind.value.trim()}>Ekle</button>
              </form>
              <ul className="max-h-56 space-y-1 overflow-y-auto text-sm">
                {draft.entities.map((e, i) => (
                  <li key={i} className="flex items-center gap-2 rounded border border-osiris-line px-2 py-1">
                    <EntityTypeTag type={e.type} />
                    <span className="min-w-0 flex-1 truncate font-mono text-xs">{e.value}</span>
                    <button className="text-xs text-osiris-muted hover:text-osiris-bad" onClick={() => set({ entities: draft.entities.filter((_, k) => k !== i) })}>Çıkar</button>
                  </li>
                ))}
              </ul>
            </section>
          </div>
        </div>

        <aside className="min-w-0">
          <div className="panel sticky top-4 p-5">
            <h2 className="mb-3 font-semibold">Üret</h2>
            <Segmented label="Biçim" value={format} onChange={(f) => { setFormat(f); setOut(null); }} options={FORMATS.map((f) => ({ value: f.value, label: f.label }))} />
            <p className="mt-2 text-xs text-osiris-faint">{FORMATS.find((f) => f.value === format)?.hint}</p>
            {format === "stix" && <p className="mt-1 text-xs text-osiris-muted">STIX'e girecek gösterge: <span className="tnum">{stixCount}</span></p>}
            {!canGen && <div className="mt-3"><Notice kind="info">Rapor üretmek için analist rolü gerekir.</Notice></div>}
            <button className="btn-primary mt-4 w-full" disabled={busy || !canGen} onClick={generate}>{busy ? "Üretiliyor" : "Raporu üret"}</button>
            {error && <div className="mt-3"><Notice>{error}</Notice></div>}
            {out && (
              <div className="mt-4 space-y-2">
                <div className="flex flex-wrap gap-2">
                  <button className="btn-ghost py-1" onClick={() => downloadText(out.filename, out.content, out.media_type)}>{out.filename} indir</button>
                  <button className="btn-ghost py-1" onClick={async () => toast((await copyText(out.content)) ? "Panoya kopyalandı" : "Kopyalanamadı", "ok")}>Kopyala</button>
                </div>
                {out.format === "html" ? (
                  <iframe title="HTML rapor önizlemesi" sandbox="" srcDoc={out.content} className="h-[55vh] w-full rounded-md border border-osiris-line bg-white" />
                ) : (
                  <pre className="max-h-[55vh] overflow-auto whitespace-pre-wrap break-words rounded-md border border-osiris-line bg-osiris-sink p-3 font-mono text-xs text-osiris-ink/90">{out.content}</pre>
                )}
              </div>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}
