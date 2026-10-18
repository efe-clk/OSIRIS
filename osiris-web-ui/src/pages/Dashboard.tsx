// Panel: sayaçlar (GET /stats), kaynak sağlığı, son öğeler, öne çıkan varlıklar, hızlı eylemler.
import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Api, can, type Source } from "../api";
import { useAuth } from "../auth";
import { ItemDrawer } from "../itemDrawer";
import { EntityTypeTag, Loading, Notice, PageHeader, Pill, Section, useLoad } from "../ui";
import { fmtAgo, fmtNum, hostOf } from "../util";

function health(s: Source): { tone: "ok" | "bad" | "warn" | "idle"; label: string } {
  if (!s.enabled) return { tone: "idle", label: "Kapalı" };
  if (!s.last_crawled_at) return { tone: "idle", label: "Hiç çalışmadı" };
  if (s.failure_count >= 3) return { tone: "bad", label: `${s.failure_count} hata` };
  if (s.failure_count > 0) return { tone: "warn", label: `${s.failure_count} hata` };
  return { tone: "ok", label: "Sağlıklı" };
}

function Stat({ label, value, sub, to }: { label: string; value: string; sub?: string; to?: string }) {
  const body = (
    <>
      <div className="text-xs text-osiris-muted">{label}</div>
      <div className="tnum mt-1 text-2xl font-semibold">{value}</div>
      {sub && <div className="mt-0.5 text-xs text-osiris-faint">{sub}</div>}
    </>
  );
  return to ? (
    <Link to={to} className="block border-osiris-line px-4 py-3 transition-colors hover:bg-osiris-raise">{body}</Link>
  ) : (
    <div className="px-4 py-3">{body}</div>
  );
}

export default function Dashboard() {
  const { role } = useAuth();
  const nav = useNavigate();
  const [q, setQ] = useState("");
  const [openId, setOpenId] = useState<string | null>(null);
  const stats = useLoad(() => Api.stats());
  const sources = useLoad(() => Api.sources({ limit: 200 }));
  const items = useLoad(() => Api.recentItems(8));
  const ents = useLoad(() => Api.topEntities(undefined, 12));

  const s = stats.data;
  const srcs = sources.data ?? [];
  const failing = srcs.filter((x) => x.enabled && x.failure_count > 0);
  const lastRun = srcs.map((x) => x.last_crawled_at).filter(Boolean).sort().pop() ?? null;

  return (
    <div>
      <PageHeader
        title="Panel"
        lede="Platformun genel durumu: toplanan veri, kaynakların sağlığı ve son bulgular."
        actions={can(role, "analyst") ? (
          <>
            <Link to="/username" className="btn-ghost">Kullanıcı adı tara</Link>
            <Link to="/collect" className="btn-primary">Toplama başlat</Link>
          </>
        ) : undefined}
      />

      <form
        onSubmit={(e) => { e.preventDefault(); if (q.trim()) nav(`/search?q=${encodeURIComponent(q.trim())}`); }}
        className="mb-6 flex gap-2"
      >
        <input
          id="dash-q"
          className="field h-11 text-base"
          placeholder="Toplanan tüm içerikte ara: alan adı, e-posta, anahtar kelime…"
          aria-label="Hızlı arama"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <button className="btn-primary h-11 px-5" disabled={!q.trim()}>Ara</button>
      </form>

      {stats.error && <Notice>{stats.error}</Notice>}
      <div className="panel mb-8 grid grid-cols-2 divide-x divide-y divide-osiris-line overflow-hidden sm:grid-cols-3 lg:grid-cols-6 lg:divide-y-0">
        <Stat label="Öğe" value={fmtNum(s?.items)} sub={s?.latest_item_at ? `son: ${fmtAgo(s.latest_item_at)}` : "henüz yok"} to="/items" />
        <Stat label="Varlık" value={fmtNum(s?.entities)} to="/entities" />
        <Stat label="Kaynak" value={fmtNum(s?.sources)} sub={s ? `${s.sources_enabled} etkin` : undefined} to="/sources" />
        <Stat label="Graf kenarı" value={fmtNum(s?.edges)} to="/graph" />
        <Stat label="Kayıtlı sorgu" value={fmtNum(s?.saved_queries)} to="/alerts" />
        <Stat label="Etkin uyarı" value={fmtNum(s?.alerts)} to="/alerts" />
      </div>

      <div className="grid gap-8 lg:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]">
        <Section title="Son toplanan öğeler" aside={<Link to="/items" className="hover:text-osiris-ink">Tümü</Link>}>
          {items.loading && !items.data ? <Loading /> : items.error ? <Notice>{items.error}</Notice> : (
            items.data && items.data.length > 0 ? (
              <ul className="panel divide-y divide-osiris-line">
                {items.data.map((it) => (
                  <li key={it.id}>
                    <button onClick={() => setOpenId(it.id)} className="block w-full px-4 py-3 text-left hover:bg-osiris-raise">
                      <div className="flex items-baseline justify-between gap-3">
                        <span className="truncate font-medium">{it.title || it.url || "Başlıksız öğe"}</span>
                        <span className="shrink-0 text-xs text-osiris-faint">{fmtAgo(it.collected_at)}</span>
                      </div>
                      <div className="mt-0.5 truncate text-xs text-osiris-muted">
                        <span className="font-mono">{hostOf(it.url)}</span>
                        {it.snippet ? ` · ${it.snippet.slice(0, 140)}` : ""}
                      </div>
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <div className="panel px-5 py-8 text-sm text-osiris-muted">
                Henüz öğe yok. Bir toplama başlatın ya da kaynak ekleyin; pipeline işçisi öğeleri işleyip buraya yazar.
              </div>
            )
          )}
        </Section>

        <div className="min-w-0 space-y-8">
          <Section
            title="Kaynak sağlığı"
            aside={<Link to="/sources" className="hover:text-osiris-ink">Kaynaklar</Link>}
          >
            {sources.loading && !sources.data ? <Loading /> : sources.error ? <Notice>{sources.error}</Notice> : (
              <div className="panel p-4">
                <div className="tnum flex flex-wrap gap-x-5 gap-y-1 text-sm">
                  <span><span className="text-osiris-ok">●</span> {srcs.filter((x) => health(x).tone === "ok").length} sağlıklı</span>
                  <span><span className="text-osiris-warn">●</span> {srcs.filter((x) => health(x).tone === "warn").length} uyarı</span>
                  <span><span className="text-osiris-bad">●</span> {srcs.filter((x) => health(x).tone === "bad").length} hatalı</span>
                  <span className="text-osiris-muted">son çalışma: {fmtAgo(lastRun)}</span>
                </div>
                {failing.length > 0 ? (
                  <ul className="mt-3 space-y-1.5 border-t border-osiris-line pt-3">
                    {failing.slice(0, 5).map((x) => (
                      <li key={x.id} className="flex items-center justify-between gap-2 text-sm">
                        <span className="truncate">{x.name}</span>
                        <Pill tone={health(x).tone}>{health(x).label}</Pill>
                      </li>
                    ))}
                  </ul>
                ) : srcs.length === 0 ? (
                  <p className="mt-3 text-sm text-osiris-muted">Kayıtlı kaynak yok. Zamanlanmış toplama için Kaynaklar sayfasından ekleyin.</p>
                ) : null}
              </div>
            )}
          </Section>

          <Section title="Öne çıkan varlıklar" aside={<Link to="/entities" className="hover:text-osiris-ink">Tümü</Link>}>
            {ents.loading && !ents.data ? <Loading /> : ents.error ? <Notice>{ents.error}</Notice> : (
              ents.data && ents.data.length > 0 ? (
                <ul className="panel divide-y divide-osiris-line">
                  {ents.data.map((e) => (
                    <li key={`${e.type}:${e.value}`}>
                      <Link
                        to={`/search?mode=entity&type=${e.type}&q=${encodeURIComponent(e.value)}`}
                        className="flex items-center gap-3 px-4 py-2 hover:bg-osiris-raise"
                      >
                        <EntityTypeTag type={e.type} />
                        <span className="min-w-0 flex-1 truncate font-mono text-[13px]">{e.value}</span>
                        <span className="tnum text-xs text-osiris-faint">{fmtNum(e.mentions)}</span>
                      </Link>
                    </li>
                  ))}
                </ul>
              ) : <p className="text-sm text-osiris-muted">Varlık yok.</p>
            )}
          </Section>
        </div>
      </div>
      <ItemDrawer id={openId} onClose={() => setOpenId(null)} onDeleted={() => { items.reload(); stats.reload(); }} />
    </div>
  );
}
