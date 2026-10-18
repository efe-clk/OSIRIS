// Öğeler: toplanan ve işlenmiş kayıtlar (GET /items/recent, sayfalı). Detay çekmecede.
import { useMemo, useState } from "react";
import { Api } from "../api";
import { ItemDrawer } from "../itemDrawer";
import { Empty, Loading, Notice, PageHeader, Pager, useLoad } from "../ui";
import { downloadText, fmtAgo, fmtDate, hostOf, toCsv } from "../util";

const LIMIT = 50;

export default function Items() {
  const [offset, setOffset] = useState(0);
  const [openId, setOpenId] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const { data, error, loading, reload } = useLoad(() => Api.recentItems(LIMIT, offset), [offset]);

  const rows = useMemo(() => {
    const f = filter.trim().toLowerCase();
    if (!data || !f) return data ?? [];
    return data.filter((i) => `${i.title} ${i.url} ${i.snippet} ${(i.tags ?? []).join(" ")}`.toLowerCase().includes(f));
  }, [data, filter]);

  return (
    <div>
      <PageHeader
        title="Öğeler"
        lede="Pipeline'ın işleyip kaydettiği içerik, en yeniden eskiye. Bir öğeye tıklayınca temizlenmiş metni, çıkarılan varlıkları ve üst verisini görürsünüz."
        actions={
          <>
            <button className="btn-ghost" onClick={reload}>Yenile</button>
            <button className="btn-ghost" disabled={!data?.length} onClick={() => downloadText("osiris-ogeler.csv", toCsv([
              ["id", "baslik", "url", "dil", "toplandi", "etiketler"],
              ...(data ?? []).map((i) => [i.id, i.title, i.url, i.language ?? "", i.collected_at ?? "", (i.tags ?? []).join(" ")]),
            ]), "text/csv")}>Bu sayfayı CSV indir</button>
          </>
        }
      />
      <input id="items-filter" className="field mb-4 max-w-md" placeholder="Bu sayfada süz (başlık, adres, etiket)" aria-label="Öğeleri süz" value={filter} onChange={(e) => setFilter(e.target.value)} />
      {error && <Notice>{error}</Notice>}
      {loading && !data ? <Loading /> : rows.length === 0 && !filter && offset === 0 ? (
        <Empty title="Henüz öğe yok">Toplama sayfasından bir plugin çalıştırın. Öğelerin burada görünmesi için pipeline işçisinin çalışıyor olması gerekir (compose <span className="font-mono">worker</span> profili).</Empty>
      ) : (
        <div className="panel overflow-x-auto">
          <table className="w-full min-w-[640px] text-sm">
            <thead>
              <tr className="border-b border-osiris-line text-left text-xs text-osiris-muted">
                <th className="px-4 py-2.5 font-medium">Başlık</th>
                <th className="px-4 py-2.5 font-medium">Kaynak adres</th>
                <th className="px-4 py-2.5 font-medium">Dil</th>
                <th className="px-4 py-2.5 font-medium">Etiketler</th>
                <th className="px-4 py-2.5 text-right font-medium">Toplandı</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-osiris-line">
              {rows.map((i) => (
                <tr key={i.id} onClick={() => setOpenId(i.id)} className="cursor-pointer hover:bg-osiris-raise">
                  <td className="max-w-[22rem] px-4 py-2.5">
                    <button className="block max-w-full truncate text-left font-medium hover:text-osiris-accent" onClick={(e) => { e.stopPropagation(); setOpenId(i.id); }}>
                      {i.title || "Başlıksız öğe"}
                    </button>
                    {i.snippet && <div className="truncate text-xs text-osiris-faint">{i.snippet.slice(0, 120)}</div>}
                  </td>
                  <td className="max-w-[14rem] truncate px-4 py-2.5 font-mono text-xs text-osiris-muted">{hostOf(i.url) || "—"}</td>
                  <td className="px-4 py-2.5 text-osiris-muted">{i.language || "—"}</td>
                  <td className="max-w-[12rem] truncate px-4 py-2.5 text-xs text-osiris-muted">{(i.tags ?? []).join(", ") || "—"}</td>
                  <td className="whitespace-nowrap px-4 py-2.5 text-right text-xs text-osiris-muted" title={fmtDate(i.collected_at)}>{fmtAgo(i.collected_at)}</td>
                </tr>
              ))}
              {rows.length === 0 && <tr><td colSpan={5} className="px-4 py-6 text-center text-osiris-muted">Bu süzgece uyan öğe yok.</td></tr>}
            </tbody>
          </table>
        </div>
      )}
      {data && <Pager offset={offset} limit={LIMIT} count={data.length} onChange={setOffset} />}
      <ItemDrawer id={openId} onClose={() => setOpenId(null)} onDeleted={reload} />
    </div>
  );
}
