// Ad hoc toplama: plugin seç → şema-güdümlü form → çalıştır (POST /collect, include_items).
// İşler kuyruğa da eklenebilir (POST /collect/batch, ≤10) veya kalıcı kaynağa çevrilebilir.
import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { Api, ApiError, can, type BatchResult, type CollectResult, type Plugin } from "../api";
import { useAuth } from "../auth";
import { useReportDraft } from "../reportDraft";
import { initialValues, SchemaForm, toConfig, URL_KEYS, type FormValues } from "../schemaForm";
import { Empty, Label, Loading, Notice, PageHeader, Pill, Spinner, useLoad, useUi } from "../ui";
import { downloadText, errMsg, hostOf, safeHref } from "../util";

export const NETWORK_LABEL: Record<string, string> = {
  www: "Açık web", tor: "Tor", i2p: "I2P", rss: "RSS/Atom", api: "API", irc: "IRC",
  matrix: "Matrix", blockchain: "Blockchain", p2p: "P2P", freenet: "Freenet",
  zeronet: "ZeroNet", sdr: "SDR", custom: "Özel",
};

interface Job { plugin_id: string; config: Record<string, unknown>; label: string }

export default function Collect() {
  const { role } = useAuth();
  const { toast } = useUi();
  const { addFinding } = useReportDraft();
  const [params] = useSearchParams();
  const plugins = useLoad(() => Api.plugins());
  const [pid, setPid] = useState<string>(params.get("plugin") ?? "");
  const [values, setValues] = useState<FormValues>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{ plugin: Plugin; config: Record<string, unknown>; r: CollectResult } | null>(null);
  const [queue, setQueue] = useState<Job[]>([]);
  const [batch, setBatch] = useState<BatchResult | null>(null);
  const [saveOpen, setSaveOpen] = useState(false);
  const ctrl = useRef<AbortController | null>(null);

  const list = useMemo(
    () => (plugins.data ?? []).filter((p) => p.id !== "username-search").sort((a, b) => a.name.localeCompare(b.name, "tr")),
    [plugins.data],
  );
  const plugin = list.find((p) => p.id === pid) ?? null;
  const canRun = can(role, "analyst");

  useEffect(() => {
    if (plugin) {
      setValues(initialValues(plugin.config_schema));
      setErrors({});
    }
  }, [plugin]);

  const build = () => {
    if (!plugin) return null;
    const { config, errors: errs } = toConfig(plugin.config_schema, values);
    setErrors(errs);
    return Object.keys(errs).length ? null : config;
  };

  const run = async () => {
    const config = build();
    if (!plugin || !config) return;
    setError(null);
    setBusy(true);
    setResult(null);
    const t0 = Date.now();
    const timer = setInterval(() => setElapsed(Math.round((Date.now() - t0) / 1000)), 1000);
    ctrl.current = new AbortController();
    try {
      const r = await Api.collect(plugin.id, config, { signal: ctrl.current.signal });
      setResult({ plugin, config, r });
      toast(`${r.items} öğe toplandı ve işleme kuyruğuna yazıldı`);
    } catch (e) {
      setError(e instanceof ApiError && e.status === 502
        ? `Plugin çalıştı ama başarısız oldu: ${e.message}`
        : errMsg(e));
    } finally {
      clearInterval(timer);
      setBusy(false);
      setElapsed(0);
    }
  };

  const enqueue = () => {
    const config = build();
    if (!plugin || !config) return;
    if (queue.length >= 10) return toast("Toplu işte en fazla 10 iş olabilir", "error");
    const first = Object.values(config).find((v) => typeof v === "string") as string | undefined;
    setQueue((q) => [...q, { plugin_id: plugin.id, config, label: `${plugin.name}${first ? ` · ${first}` : ""}` }]);
  };

  const runBatch = async () => {
    setBusy(true);
    setError(null);
    setBatch(null);
    try {
      const r = await Api.collectBatch(queue.map(({ plugin_id, config }) => ({ plugin_id, config })));
      setBatch(r);
      toast(`${r.ok}/${r.total} iş başarılı`);
      setQueue([]);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  if (plugins.loading && !plugins.data) return <Loading />;

  return (
    <div>
      <PageHeader
        title="Toplama"
        lede="Bir collector plugin'ini tek seferlik çalıştırın. Toplanan öğeler işleme kuyruğuna yazılır; pipeline temizler, varlıkları çıkarır ve veritabanına kaydeder. Düzenli toplama için işi kaynak olarak kaydedin."
        actions={<Link to="/username" className="btn-ghost">Kullanıcı adı tarama</Link>}
      />
      {!canRun && <Notice kind="info">Toplama başlatmak için analist rolü gerekir.</Notice>}
      {plugins.error && <Notice>{plugins.error}</Notice>}

      <div className="grid gap-6 lg:grid-cols-[17rem_minmax(0,1fr)]">
        <nav aria-label="Plugin'ler" className="space-y-1">
          <div className="mb-2 text-xs text-osiris-faint">{list.length} collector</div>
          {list.map((p) => (
            <button
              key={p.id}
              onClick={() => { setPid(p.id); setResult(null); setError(null); }}
              aria-current={p.id === pid}
              className={`block w-full rounded-md border px-3 py-2 text-left transition-colors ${
                p.id === pid ? "border-osiris-accent bg-osiris-accent/10" : "border-transparent hover:border-osiris-line hover:bg-osiris-panel"
              }`}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="truncate text-sm font-medium">{p.name}</span>
                <span className="shrink-0 text-[11px] text-osiris-faint">{NETWORK_LABEL[p.network_type] ?? p.network_type}</span>
              </div>
              <div className="truncate font-mono text-[11px] text-osiris-faint">{p.id}</div>
            </button>
          ))}
        </nav>

        <div className="min-w-0 space-y-5">
          {!plugin ? (
            <Empty title="Bir plugin seçin">
              Soldaki listeden bir collector seçin. Ayar formu plugin'in manifest'indeki şemadan üretilir.
            </Empty>
          ) : (
            <div className="panel p-5">
              <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0">
                  <h2 className="text-lg font-semibold">{plugin.name}</h2>
                  <p className="mt-0.5 max-w-[65ch] text-sm text-osiris-muted">{plugin.description || "Açıklama yok."}</p>
                </div>
                <div className="flex flex-wrap gap-1.5">
                  <Pill tone="accent">{NETWORK_LABEL[plugin.network_type] ?? plugin.network_type}</Pill>
                  {plugin.schedule_default && <Pill>varsayılan cron <span className="font-mono">{plugin.schedule_default}</span></Pill>}
                </div>
              </div>
              {(plugin.network_type === "tor" || plugin.network_type === "i2p") && (
                <Notice kind="warn">
                  Bu plugin operatörün tanımladığı {plugin.network_type === "tor" ? "Tor SOCKS5" : "I2P HTTP"} proxy'si üzerinden çalışır
                  (compose <span className="font-mono">darknet</span> profili). Proxy çalışmıyorsa toplama 502 ile başarısız olur.
                </Notice>
              )}
              <SchemaForm schema={plugin.config_schema} values={values} onChange={setValues} errors={errors} idPrefix={`c-${plugin.id}`} />
              <div className="mt-5 flex flex-wrap gap-2 border-t border-osiris-line pt-4">
                <button className="btn-primary" disabled={busy || !canRun} onClick={run}>
                  {busy && !queue.length ? "Çalışıyor" : "Şimdi çalıştır"}
                </button>
                <button className="btn-ghost" disabled={busy || !canRun} onClick={enqueue}>Toplu işe ekle</button>
                <button className="btn-ghost" disabled={!canRun} onClick={() => { if (build()) setSaveOpen(true); }}>
                  Kaynak olarak kaydet
                </button>
              </div>
            </div>
          )}

          {busy && (
            <div className="panel flex flex-wrap items-center justify-between gap-3 px-5 py-4">
              <Spinner label={`Plugin çalışıyor · ${elapsed} sn`} />
              {!queue.length && <button className="btn-ghost" onClick={() => ctrl.current?.abort()}>Beklemeyi bırak</button>}
            </div>
          )}
          {error && <Notice onClose={() => setError(null)}>{error}</Notice>}

          {queue.length > 0 && (
            <div className="panel p-5">
              <div className="mb-3 flex items-center justify-between gap-3">
                <h3 className="font-semibold">Toplu iş <span className="tnum text-osiris-faint">{queue.length}/10</span></h3>
                <div className="flex gap-2">
                  <button className="btn-ghost py-1" onClick={() => setQueue([])}>Temizle</button>
                  <button className="btn-primary py-1" disabled={busy} onClick={runBatch}>Hepsini çalıştır</button>
                </div>
              </div>
              <ol className="space-y-1.5 text-sm">
                {queue.map((j, i) => (
                  <li key={i} className="flex items-center justify-between gap-3 rounded-md border border-osiris-line px-3 py-2">
                    <span className="min-w-0 truncate"><span className="tnum text-osiris-faint">{i + 1}.</span> {j.label}</span>
                    <button className="text-xs text-osiris-muted hover:text-osiris-bad" onClick={() => setQueue((q) => q.filter((_, k) => k !== i))}>Çıkar</button>
                  </li>
                ))}
              </ol>
              <p className="mt-2 text-xs text-osiris-faint">İşler sunucuda sırayla çalışır; biri başarısız olsa da diğerleri devam eder.</p>
            </div>
          )}

          {batch && (
            <div className="panel p-5">
              <h3 className="mb-3 font-semibold">Toplu iş sonucu · <span className="tnum">{batch.ok}/{batch.total}</span> başarılı</h3>
              <ul className="space-y-1.5 text-sm">
                {batch.jobs.map((j, i) => (
                  <li key={i} className="flex items-start gap-3">
                    <Pill tone={j.ok ? "ok" : "bad"}>{j.ok ? `${j.items ?? 0} öğe` : "başarısız"}</Pill>
                    <span className="font-mono text-[13px]">{j.plugin_id}</span>
                    {j.error && <span className="min-w-0 break-words text-osiris-bad">{j.error}</span>}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {result && <CollectPreview result={result} onReport={() => {
            addFinding({
              title: `${result.plugin.name}: ${result.r.items} öğe toplandı`,
              description: (result.r.results ?? []).slice(0, 30).map((p) => `${p.title || p.url || "öğe"}${p.url ? ` — ${p.url}` : ""}`).join("\n"),
            });
            toast("Sonuç rapor taslağına eklendi");
          }} />}
        </div>
      </div>

      {saveOpen && plugin && (
        <SaveAsSource
          plugin={plugin}
          config={toConfig(plugin.config_schema, values).config}
          onClose={() => setSaveOpen(false)}
        />
      )}
    </div>
  );
}

function CollectPreview({ result, onReport }: {
  result: { plugin: Plugin; config: Record<string, unknown>; r: CollectResult }; onReport: () => void;
}) {
  const { r, plugin } = result;
  const rows = r.results ?? [];
  const meta = Object.entries(r.metadata ?? {}).filter(([, v]) => typeof v !== "object");
  return (
    <section className="space-y-3">
      <div className="flex flex-wrap items-end justify-between gap-3 border-b border-osiris-line pb-3">
        <div>
          <h3 className="text-lg font-semibold"><span className="tnum">{r.items}</span> öğe toplandı</h3>
          {meta.length > 0 && (
            <p className="mt-0.5 text-xs text-osiris-muted">
              {meta.map(([k, v]) => <span key={k} className="mr-3"><span className="text-osiris-faint">{k}</span> <span className="tnum">{String(v)}</span></span>)}
            </p>
          )}
        </div>
        <div className="flex gap-2">
          <button className="btn-ghost py-1" disabled={!rows.length}
            onClick={() => downloadText(`${plugin.id}-sonuc.json`, JSON.stringify(r, null, 2), "application/json")}>JSON indir</button>
          <button className="btn-ghost py-1" disabled={!r.items} onClick={onReport}>Rapora ekle</button>
        </div>
      </div>
      {rows.length === 0 ? (
        <p className="text-sm text-osiris-muted">Önizlenecek öğe yok.</p>
      ) : (
        <ul className="space-y-2">
          {rows.slice(0, 200).map((p, i) => {
            const href = safeHref(p.url);
            return (
              <li key={i} className="panel px-4 py-3">
                <div className="flex items-baseline justify-between gap-3">
                  <span className="min-w-0 truncate font-medium">{p.title || hostOf(p.url) || "Başlıksız"}</span>
                  {p.tags.length > 0 && <span className="shrink-0 text-[11px] text-osiris-faint">{p.tags.slice(0, 3).join(" · ")}</span>}
                </div>
                {href && <a href={href} target="_blank" rel="noopener noreferrer" className="block truncate font-mono text-xs text-osiris-accent hover:underline">{p.url}</a>}
                {p.snippet && <p className="mt-1 line-clamp-3 break-words text-sm text-osiris-muted">{p.snippet}</p>}
              </li>
            );
          })}
          {rows.length > 200 && <li className="text-sm text-osiris-faint">İlk 200 öğe gösteriliyor.</li>}
        </ul>
      )}
      <p className="text-xs text-osiris-faint">Öğeler pipeline işçisi çalışıyorsa birkaç saniye içinde Öğeler ve Arama sayfalarında görünür.</p>
    </section>
  );
}

function SaveAsSource({ plugin, config, onClose }: { plugin: Plugin; config: Record<string, unknown>; onClose: () => void }) {
  const { toast } = useUi();
  const urlKey = URL_KEYS.find((k) => typeof config[k] === "string");
  const [name, setName] = useState(urlKey ? `${plugin.name} · ${hostOf(String(config[urlKey]))}` : plugin.name);
  const [schedule, setSchedule] = useState(plugin.schedule_default || "");
  const [retention, setRetention] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await Api.createSource({
        name: name.trim(), plugin_id: plugin.id, network_type: plugin.network_type,
        url: urlKey ? String(config[urlKey]) : "", schedule: schedule.trim(), config,
        retention_days: retention.trim() === "" ? null : parseInt(retention, 10),
      });
      toast("Kaynak kaydedildi");
      onClose();
    } catch (err) {
      setError(errMsg(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4" role="dialog" aria-modal="true">
      <button className="absolute inset-0 bg-black/70" onClick={onClose} aria-label="Kapat" />
      <form onSubmit={save} className="panel relative w-full max-w-lg space-y-4 p-5">
        <h2 className="text-lg font-semibold">Kaynak olarak kaydet</h2>
        <Label label="Ad" required htmlFor="sv-name">
          <input id="sv-name" className="field" value={name} onChange={(e) => setName(e.target.value)} />
        </Label>
        <div className="grid grid-cols-2 gap-4">
          <Label label="Zamanlama (cron)" hint="5 alan; boş = yalnızca elle" htmlFor="sv-cron">
            <input id="sv-cron" className="field font-mono" value={schedule} onChange={(e) => setSchedule(e.target.value)} placeholder="*/15 * * * *" />
          </Label>
          <Label label="Saklama (gün)" hint="Boş = sunucu varsayılanı, 0 = silinmez" htmlFor="sv-ret">
            <input id="sv-ret" className="field tnum" inputMode="numeric" value={retention} onChange={(e) => setRetention(e.target.value.replace(/\D/g, ""))} />
          </Label>
        </div>
        {error && <p className="text-sm text-osiris-bad">{error}</p>}
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>Vazgeç</button>
          <button className="btn-primary" disabled={busy || !name.trim()}>{busy ? "Kaydediliyor" : "Kaydet"}</button>
        </div>
      </form>
    </div>
  );
}
