// Varlıklar: en çok anılan e-posta, IP, alan adı, kişi… (GET /entities/top).
// Her satırdan: ilgili öğeleri ara, grafa bağla, STIX'e/rapora ekle, (admin) KVKK silme.
import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { Api, can, ENTITY_LABEL, ENTITY_TYPES, type TopEntity } from "../api";
import { useAuth } from "../auth";
import { useReportDraft } from "../reportDraft";
import { Empty, EntityTypeTag, Loading, Notice, PageHeader, useLoad, useUi } from "../ui";
import { copyText, downloadText, errMsg, fmtAgo, fmtNum, toCsv } from "../util";

export default function Entities() {
  const { role } = useAuth();
  const { confirm, toast } = useUi();
  const { addFinding, addEntities, draft } = useReportDraft();
  const [type, setType] = useState<string>("");
  const [limit, setLimit] = useState(100);
  const [filter, setFilter] = useState("");
  const { data, error, loading, reload } = useLoad(() => Api.topEntities(type || undefined, limit), [type, limit]);

  const rows = useMemo(() => {
    const f = filter.trim().toLowerCase();
    return (data ?? []).filter((e) => !f || e.value.toLowerCase().includes(f));
  }, [data, filter]);
  const max = Math.max(1, ...rows.map((r) => r.mentions));
  const inDraft = new Set(draft.entities.map((e) => `${e.type}:${e.value.toLowerCase()}`));

  const erase = async (e: TopEntity) => {
    const ok = await confirm({
      title: `“${e.value}” kalıcı olarak silinsin mi?`,
      body: (
        <>
          <p>KVKK silme: bu varlık, onu anan <b>tüm öğeler</b> ({fmtNum(e.mentions)} anma) ve aynı adlı graf kenarları silinir.</p>
          <p className="mt-2">Denetim kaydına değerin kendisi değil, yalnızca SHA-256 özeti yazılır. Geri alınamaz.</p>
        </>
      ),
      confirmLabel: "Varlığı ve öğeleri sil",
      danger: true,
    });
    if (!ok) return;
    try {
      const r = await Api.eraseEntity(e.type, e.value, true);
      toast(`${r.items_deleted} öğe, ${r.entities_deleted} varlık, ${r.edges_deleted} kenar silindi`);
      reload();
    } catch (err) {
      toast(errMsg(err), "error");
    }
  };

  return (
    <div>
      <PageHeader
        title="Varlıklar"
        lede="Toplanan içerikten çıkarılan göstergeler, anma sayısına göre. Regex e-posta, IP, alan adı, telefon, kripto adresi ve CVE bulur; NER modeli kişi, kurum ve yer ekler."
        actions={
          <button className="btn-ghost" disabled={!rows.length} onClick={() => downloadText("osiris-varliklar.csv",
            toCsv([["tur", "deger", "anma", "son_gorulme"], ...rows.map((e) => [e.type, e.value, e.mentions, e.last_seen ?? ""])]), "text/csv")}>
            CSV indir
          </button>
        }
      />
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <div className="flex flex-wrap gap-1.5" role="group" aria-label="Varlık türü">
          {["", ...ENTITY_TYPES].map((t) => (
            <button key={t || "all"} onClick={() => setType(t)} aria-pressed={type === t}
              className={`rounded-full border px-3 py-1 text-sm ${type === t ? "border-osiris-accent bg-osiris-accent/10 text-osiris-accent" : "border-osiris-line text-osiris-muted hover:text-osiris-ink"}`}>
              {t ? ENTITY_LABEL[t as keyof typeof ENTITY_LABEL] : "Tümü"}
            </button>
          ))}
        </div>
      </div>
      <div className="mb-4 flex flex-wrap gap-2">
        <input id="ent-filter" className="field max-w-sm" placeholder="Değerde süz" aria-label="Varlıkları süz" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <select id="ent-limit" aria-label="Gösterilecek sayı" className="field w-32" value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
          {[20, 50, 100].map((n) => <option key={n} value={n}>İlk {n}</option>)}
        </select>
      </div>
      {error && <Notice>{error}</Notice>}
      {loading && !data ? <Loading /> : rows.length === 0 ? (
        <Empty title="Varlık bulunamadı">{type ? "Bu türde varlık yok." : "Öğeler işlendikçe varlıklar burada birikir."}</Empty>
      ) : (
        <div className="panel overflow-x-auto">
          <table className="w-full min-w-[680px] text-sm">
            <thead>
              <tr className="border-b border-osiris-line text-left text-xs text-osiris-muted">
                <th className="px-4 py-2.5 font-medium">Tür</th>
                <th className="px-4 py-2.5 font-medium">Değer</th>
                <th className="w-48 px-4 py-2.5 font-medium">Anma</th>
                <th className="px-4 py-2.5 font-medium">Son görülme</th>
                <th className="px-4 py-2.5 text-right font-medium">Eylemler</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-osiris-line">
              {rows.map((e) => {
                const key = `${e.type}:${e.value.toLowerCase()}`;
                return (
                  <tr key={key} className="hover:bg-osiris-raise">
                    <td className="px-4 py-2"><EntityTypeTag type={e.type} /></td>
                    <td className="max-w-[20rem] px-4 py-2">
                      <button className="block max-w-full truncate font-mono text-[13px] hover:text-osiris-accent" title="Kopyala"
                        onClick={async () => toast((await copyText(e.value)) ? "Kopyalandı" : "Kopyalanamadı", "ok")}>{e.value}</button>
                    </td>
                    <td className="px-4 py-2">
                      <div className="flex items-center gap-2">
                        <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-osiris-sink">
                          <div className="h-full rounded-full bg-osiris-accent/70" style={{ width: `${(e.mentions / max) * 100}%` }} />
                        </div>
                        <span className="tnum w-10 text-right text-xs text-osiris-muted">{fmtNum(e.mentions)}</span>
                      </div>
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-xs text-osiris-muted">{fmtAgo(e.last_seen)}</td>
                    <td className="whitespace-nowrap px-4 py-2 text-right">
                      <div className="inline-flex gap-3 text-xs">
                        <Link className="text-osiris-accent hover:underline" to={`/search?mode=entity&type=${e.type}&q=${encodeURIComponent(e.value)}`}>Öğeler</Link>
                        <Link className="text-osiris-muted hover:text-osiris-ink" to={`/graph?node=${encodeURIComponent(e.value)}`}>Graf</Link>
                        <button className="text-osiris-muted hover:text-osiris-ink disabled:opacity-40" disabled={inDraft.has(key)} onClick={() => {
                          addEntities([{ type: e.type, value: e.value }]);
                          addFinding({ title: `${ENTITY_LABEL[e.type]}: ${e.value}`, description: `${e.mentions} öğede anıldı, son görülme ${fmtAgo(e.last_seen)}.` });
                          toast("Rapor taslağına gösterge olarak eklendi");
                        }}>{inDraft.has(key) ? "Raporda" : "Rapora"}</button>
                        {can(role, "admin") && <button className="text-osiris-bad/80 hover:text-osiris-bad" onClick={() => erase(e)}>Sil</button>}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
