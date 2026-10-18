// Maigret tarzı kullanıcı adı taraması: username-search plugin'i (POST /collect, include_items).
// Bulunan profiller pipeline'a da düşer; burada anında listelenir, dışa aktarılır, grafa bağlanır.
import { useMemo, useRef, useState } from "react";
import { Api, ApiError, can, type CollectResult } from "../api";
import { useAuth } from "../auth";
import { useReportDraft } from "../reportDraft";
import { Empty, Label, Notice, PageHeader, Pill, Segmented, Spinner, useUi } from "../ui";
import { downloadText, errMsg, hostOf, safeHref, toCsv } from "../util";

const USERNAME_RE = /^[A-Za-z0-9_][A-Za-z0-9_.-]{1,63}$/;

const TAGS: { id: string; label: string }[] = [
  { id: "social", label: "Sosyal" },
  { id: "dev", label: "Yazılım" },
  { id: "forum", label: "Forum" },
  { id: "gaming", label: "Oyun" },
  { id: "photo", label: "Fotoğraf" },
  { id: "video", label: "Video" },
  { id: "music", label: "Müzik" },
  { id: "blog", label: "Blog" },
  { id: "tech", label: "Teknoloji" },
  { id: "business", label: "İş" },
  { id: "art", label: "Sanat" },
  { id: "news", label: "Haber" },
  { id: "finance", label: "Finans" },
  { id: "shopping", label: "Alışveriş" },
];

type Scope = "quick" | "full";

interface Profile { site: string; url: string; tags: string[]; check: string }
interface Run {
  username: string;
  scope: Scope;
  startedAt: number;
  seconds: number;
  checked: number;
  unknown: number;
  profiles: Profile[];
}

function parse(username: string, scope: Scope, startedAt: number, r: CollectResult): Run {
  const profiles: Profile[] = (r.results ?? []).map((p) => ({
    site: String(p.metadata?.site ?? hostOf(p.url)),
    url: p.url ?? "",
    tags: p.tags.filter((t) => t !== "username"),
    check: String(p.metadata?.check ?? ""),
  }));
  profiles.sort((a, b) => a.site.localeCompare(b.site, "tr"));
  return {
    username,
    scope,
    startedAt,
    seconds: Math.round((Date.now() - startedAt) / 1000),
    checked: Number(r.metadata.checked ?? 0),
    unknown: Number(r.metadata.unknown ?? 0),
    profiles,
  };
}

export default function UsernameScan() {
  const { role } = useAuth();
  const { toast } = useUi();
  const { addFinding } = useReportDraft();
  const [username, setUsername] = useState("");
  const [scope, setScope] = useState<Scope>("quick");
  const [tags, setTags] = useState<string[]>([]);
  const [showAdv, setShowAdv] = useState(false);
  const [maxSites, setMaxSites] = useState("500");
  const [rate, setRate] = useState("10");
  const [timeout, setTimeoutS] = useState("15");
  const [workers, setWorkers] = useState("10");
  const [busy, setBusy] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [runs, setRuns] = useState<Run[]>([]);
  const [active, setActive] = useState(0);
  const [filter, setFilter] = useState("");
  const [tagFilter, setTagFilter] = useState<string | null>(null);
  const ctrl = useRef<AbortController | null>(null);

  const canRun = can(role, "analyst");
  const valid = USERNAME_RE.test(username.trim());
  const run = runs[active];

  const start = async (e: React.FormEvent) => {
    e.preventDefault();
    const u = username.trim();
    if (!valid) return;
    setError(null);
    setBusy(true);
    setElapsed(0);
    const t0 = Date.now();
    const timer = setInterval(() => setElapsed(Math.round((Date.now() - t0) / 1000)), 1000);
    ctrl.current = new AbortController();
    const config: Record<string, unknown> = {
      username: u,
      full: scope === "full",
      max_sites: Math.max(1, Math.min(parseInt(maxSites, 10) || 200, 3000)),
      rate_per_sec: Math.max(0, Math.min(Number(rate) || 10, 50)),
      timeout: Math.max(5, Math.min(parseInt(timeout, 10) || 15, 30)),
      workers: Math.max(1, Math.min(parseInt(workers, 10) || 10, 20)),
    };
    if (scope === "quick") config.max_sites = 3000; // hızlı küme zaten küçük: tümünü tara
    if (tags.length) config.tags = tags;
    try {
      const r = await Api.collect("username-search", config, { signal: ctrl.current.signal, long: true });
      const parsed = parse(u, scope, t0, r);
      setRuns((rs) => [parsed, ...rs].slice(0, 10));
      setActive(0);
      setFilter("");
      setTagFilter(null);
      toast(`${parsed.profiles.length} profil bulundu, ${parsed.checked} site yoklandı`);
    } catch (err) {
      if (err instanceof ApiError && err.status === 404) {
        setError("username-search plugin'i sunucuda yüklü değil. plugins/username-search dizinini ve bağımlılıklarını kontrol edin.");
      } else setError(errMsg(err));
    } finally {
      clearInterval(timer);
      setBusy(false);
      ctrl.current = null;
    }
  };

  const shown = useMemo(() => {
    if (!run) return [];
    const f = filter.trim().toLowerCase();
    return run.profiles.filter(
      (p) => (!f || p.site.toLowerCase().includes(f) || p.url.toLowerCase().includes(f)) &&
        (!tagFilter || p.tags.includes(tagFilter)),
    );
  }, [run, filter, tagFilter]);

  const tagCounts = useMemo(() => {
    const m = new Map<string, number>();
    run?.profiles.forEach((p) => p.tags.forEach((t) => m.set(t, (m.get(t) ?? 0) + 1)));
    return [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12);
  }, [run]);

  const linkToGraph = async () => {
    if (!run) return;
    let ok = 0;
    for (const p of run.profiles.slice(0, 200)) {
      try {
        await Api.addRelation(run.username, p.site, "has_account");
        ok++;
      } catch {
        /* tek kenar hatası diğerlerini durdurmaz */
      }
    }
    toast(`${ok} hesap ilişkisi grafa eklendi`);
  };

  const toReport = () => {
    if (!run) return;
    addFinding({
      title: `"${run.username}" kullanıcı adı ${run.profiles.length} sitede bulundu`,
      description: run.profiles.map((p) => `${p.site}: ${p.url}`).join("\n"),
    });
    toast("Tarama sonucu rapor taslağına eklendi");
  };

  const exportCsv = () => {
    if (!run) return;
    downloadText(`${run.username}-profiller.csv`,
      toCsv([["site", "url", "kategori", "tespit"], ...run.profiles.map((p) => [p.site, p.url, p.tags.join(" "), p.check])]),
      "text/csv");
  };
  const exportJson = () => {
    if (!run) return;
    downloadText(`${run.username}-profiller.json`, JSON.stringify({
      username: run.username, scanned_at: new Date(run.startedAt).toISOString(),
      checked: run.checked, found: run.profiles.length, profiles: run.profiles,
    }, null, 2), "application/json");
  };

  return (
    <div>
      <PageHeader
        title="Kullanıcı adı tarama"
        lede="Bir kullanıcı adının hangi sitelerde hesabı olduğunu yoklar. Hızlı tarama yaygın 59 siteyi, tam tarama Maigret ve WhatsMyName birleşik veritabanındaki 2500'den fazla siteyi kapsar. Bulunan profiller öğe olarak da kaydedilir."
      />

      {!canRun && <Notice kind="info">Tarama başlatmak için analist rolü gerekir. Hesabınız izleyici yetkisinde.</Notice>}

      <form onSubmit={start} className="panel p-5">
        <div className="flex flex-col gap-3 sm:flex-row">
          <div className="relative min-w-0 flex-1">
            <span className="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 font-mono text-lg text-osiris-faint">@</span>
            <input
              id="username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="kullanici_adi"
              aria-label="Kullanıcı adı"
              autoComplete="off"
              spellCheck={false}
              disabled={busy || !canRun}
              className="field h-12 pl-10 font-mono text-lg"
            />
          </div>
          <button type="submit" disabled={busy || !valid || !canRun} className="btn-primary h-12 px-6 text-base">
            {busy ? "Taranıyor" : "Tara"}
          </button>
        </div>
        {username && !valid && (
          <p className="mt-2 text-xs text-osiris-warn">
            Kullanıcı adı 2–64 karakter olmalı; harf, rakam, alt çizgi, nokta ve tire içerebilir.
          </p>
        )}

        <div className="mt-4 flex flex-wrap items-center gap-x-5 gap-y-3">
          <Segmented
            label="Kapsam"
            value={scope}
            onChange={setScope}
            options={[{ value: "quick", label: "Hızlı · 59 site" }, { value: "full", label: "Tam · 2500+ site" }]}
          />
          <button type="button" className="text-sm text-osiris-muted hover:text-osiris-ink" onClick={() => setShowAdv((v) => !v)} aria-expanded={showAdv}>
            {showAdv ? "Gelişmiş ayarları gizle" : "Gelişmiş ayarlar"}
          </button>
        </div>

        <fieldset className="mt-4">
          <legend className="mb-2 text-sm text-osiris-muted">Kategoriler <span className="text-osiris-faint">(boş bırakırsanız hepsi)</span></legend>
          <div className="flex flex-wrap gap-1.5">
            {TAGS.map((t) => {
              const on = tags.includes(t.id);
              return (
                <button
                  key={t.id}
                  type="button"
                  aria-pressed={on}
                  onClick={() => setTags((ts) => (on ? ts.filter((x) => x !== t.id) : [...ts, t.id]))}
                  className={`rounded-full border px-3 py-1 text-sm transition-colors ${
                    on ? "border-osiris-accent bg-osiris-accent/10 text-osiris-accent" : "border-osiris-line text-osiris-muted hover:text-osiris-ink"
                  }`}
                >
                  {t.label}
                </button>
              );
            })}
          </div>
        </fieldset>

        {showAdv && (
          <div className="mt-5 grid grid-cols-2 gap-4 border-t border-osiris-line pt-4 md:grid-cols-4">
            <Label label="En çok site" hint="Tam taramada üst sınır (1–3000)" htmlFor="u-max">
              <input id="u-max" className="field tnum" inputMode="numeric" value={maxSites} onChange={(e) => setMaxSites(e.target.value)} disabled={scope === "quick"} />
            </Label>
            <Label label="İstek/sn" hint="Toplam tempo (0 = sınırsız, ≤50)" htmlFor="u-rate">
              <input id="u-rate" className="field tnum" inputMode="decimal" value={rate} onChange={(e) => setRate(e.target.value)} />
            </Label>
            <Label label="Zaman aşımı (sn)" hint="Site başına, 5–30" htmlFor="u-to">
              <input id="u-to" className="field tnum" inputMode="numeric" value={timeout} onChange={(e) => setTimeoutS(e.target.value)} />
            </Label>
            <Label label="Eşzamanlı iş" hint="1–20" htmlFor="u-w">
              <input id="u-w" className="field tnum" inputMode="numeric" value={workers} onChange={(e) => setWorkers(e.target.value)} />
            </Label>
          </div>
        )}
      </form>

      {busy && (
        <div className="panel mt-4 flex flex-wrap items-center justify-between gap-3 px-5 py-4">
          <div>
            <Spinner label={`@${username.trim()} taranıyor · ${elapsed} sn`} />
            <p className="mt-1 text-xs text-osiris-faint">
              {scope === "full"
                ? "Tam tarama site başına tempo sınırı nedeniyle birkaç dakika sürebilir. Sayfadan ayrılmayın."
                : "Hızlı tarama genellikle 10–30 saniye sürer."}
            </p>
          </div>
          <button className="btn-ghost" onClick={() => ctrl.current?.abort()}>Beklemeyi bırak</button>
        </div>
      )}

      {error && <div className="mt-4"><Notice onClose={() => setError(null)}>{error}</Notice></div>}

      {runs.length > 1 && (
        <div className="mt-6 flex flex-wrap gap-2" aria-label="Bu oturumdaki taramalar">
          {runs.map((r, i) => (
            <button
              key={r.startedAt}
              onClick={() => setActive(i)}
              className={`rounded-md border px-2.5 py-1 font-mono text-[13px] ${i === active ? "border-osiris-accent text-osiris-accent" : "border-osiris-line text-osiris-muted hover:text-osiris-ink"}`}
            >
              @{r.username} <span className="tnum text-osiris-faint">{r.profiles.length}</span>
            </button>
          ))}
        </div>
      )}

      {run ? (
        <section className="mt-6">
          <div className="flex flex-wrap items-end justify-between gap-4 border-b border-osiris-line pb-4">
            <div>
              <h2 className="font-mono text-2xl">@{run.username}</h2>
              <p className="tnum mt-1 text-sm text-osiris-muted">
                <span className="text-osiris-ok">{run.profiles.length} profil bulundu</span>
                {" · "}{run.checked} site yoklandı · {run.unknown} bulunamadı ya da yanıtsız · {run.seconds} sn
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              <button className="btn-ghost" onClick={exportCsv} disabled={!run.profiles.length}>CSV indir</button>
              <button className="btn-ghost" onClick={exportJson} disabled={!run.profiles.length}>JSON indir</button>
              <button className="btn-ghost" onClick={linkToGraph} disabled={!run.profiles.length}>Grafa bağla</button>
              <button className="btn-ghost" onClick={toReport} disabled={!run.profiles.length}>Rapora ekle</button>
            </div>
          </div>

          {run.profiles.length > 0 && (
            <div className="mt-4 flex flex-wrap items-center gap-3">
              <input
                className="field max-w-xs"
                placeholder="Sitelerde süz"
                aria-label="Sonuçları süz"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              />
              <div className="flex flex-wrap gap-1.5">
                {tagCounts.map(([t, n]) => (
                  <button
                    key={t}
                    onClick={() => setTagFilter(tagFilter === t ? null : t)}
                    aria-pressed={tagFilter === t}
                    className={`rounded-full border px-2.5 py-0.5 text-xs ${tagFilter === t ? "border-osiris-accent text-osiris-accent" : "border-osiris-line text-osiris-muted"}`}
                  >
                    {t} <span className="tnum text-osiris-faint">{n}</span>
                  </button>
                ))}
              </div>
            </div>
          )}

          {run.profiles.length === 0 ? (
            <div className="mt-4">
              <Empty title="Hiçbir sitede hesap bulunamadı">
                Tam taramayı deneyin ya da kategori filtresini kaldırın. Bazı siteler tempo sınırı nedeniyle yanıt vermemiş olabilir.
              </Empty>
            </div>
          ) : (
            <ul className="mt-4 grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3">
              {shown.map((p) => {
                const href = safeHref(p.url);
                return (
                  <li key={p.site + p.url} className="group flex min-w-0 items-start gap-3 rounded-md border border-osiris-line bg-osiris-panel px-3.5 py-3 hover:border-osiris-faint">
                    <span className="mt-1.5 h-2 w-2 shrink-0 rounded-full bg-osiris-ok" aria-hidden />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline justify-between gap-2">
                        <span className="truncate font-medium">{p.site}</span>
                        {p.check && <span className="shrink-0 text-[11px] text-osiris-faint">{p.check}</span>}
                      </div>
                      {href ? (
                        <a href={href} target="_blank" rel="noopener noreferrer" className="block truncate font-mono text-xs text-osiris-accent hover:underline">
                          {p.url.replace(/^https?:\/\//, "")}
                        </a>
                      ) : (
                        <span className="block truncate font-mono text-xs text-osiris-muted">{p.url}</span>
                      )}
                      {p.tags.length > 0 && <div className="mt-1 truncate text-[11px] text-osiris-faint">{p.tags.join(" · ")}</div>}
                    </div>
                  </li>
                );
              })}
              {shown.length === 0 && <li className="text-sm text-osiris-muted">Bu süzgece uyan profil yok.</li>}
            </ul>
          )}
        </section>
      ) : (
        !busy && (
          <div className="mt-8 grid gap-4 text-sm text-osiris-muted md:grid-cols-3">
            <div>
              <p className="font-medium text-osiris-ink">Nasıl yoklanır</p>
              <p className="mt-1">Her site için profil adresi istenir; durum kodu, sayfa metni ya da yönlendirme ile hesabın varlığı anlaşılır.</p>
            </div>
            <div>
              <p className="font-medium text-osiris-ink">Tempo ve güvenlik</p>
              <p className="mt-1">İstekler SSRF denetiminden geçer, aynı siteye saniyede bir istek gönderilir. Toplam hız gelişmiş ayarlardan değişir.</p>
            </div>
            <div>
              <p className="font-medium text-osiris-ink">Sonrası</p>
              <p className="mt-1">Sonuçları CSV ya da JSON olarak indirin, kullanıcı adını bulunan sitelere grafta bağlayın veya rapora bulgu olarak ekleyin.</p>
            </div>
          </div>
        )
      )}
      {run && <p className="mt-6 text-xs text-osiris-faint"><Pill tone="idle">Not</Pill> Bulunan profiller pipeline işçisi çalışıyorsa Öğeler sayfasında da görünür.</p>}
    </div>
  );
}
