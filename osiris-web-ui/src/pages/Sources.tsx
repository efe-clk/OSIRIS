// Kaynaklar: zamanlanmış toplama tanımları. Liste + sağlık (GET /sources), oluştur (POST),
// düzenle / etkinleştir (GET + PATCH /sources/{id}), şimdi çalıştır (POST …/collect), sil.
import { useEffect, useMemo, useState } from "react";
import { Api, can, NETWORK_TYPES, type Plugin, type Source, type SourceDetail } from "../api";
import { useAuth } from "../auth";
import {
  initialValues, SchemaForm, toConfig, toConfigForUpdate, URL_KEYS, valuesFromConfig, type FormValues,
} from "../schemaForm";
import { Drawer, Empty, KeyValue, Label, Loading, Notice, PageHeader, Pill, Segmented, Toggle, useLoad, useUi } from "../ui";
import { errMsg, fmtAgo, fmtDate, safeHref } from "../util";
import { NETWORK_LABEL } from "./Collect";

function health(s: Source): { tone: "ok" | "bad" | "warn" | "idle"; label: string } {
  if (!s.enabled) return { tone: "idle", label: "Kapalı" };
  if (!s.last_crawled_at) return { tone: "idle", label: "Bekliyor" };
  if (s.failure_count >= 3) return { tone: "bad", label: `${s.failure_count} ardışık hata` };
  if (s.failure_count > 0) return { tone: "warn", label: `${s.failure_count} hata` };
  return { tone: "ok", label: "Sağlıklı" };
}

const CRON_PRESETS: { label: string; value: string }[] = [
  { label: "Elle", value: "" },
  { label: "15 dk", value: "*/15 * * * *" },
  { label: "Saatlik", value: "0 * * * *" },
  { label: "6 saatte bir", value: "0 */6 * * *" },
  { label: "Günlük 03:00", value: "0 3 * * *" },
];

const CRON_RE = /^(\S+\s+){4}\S+$/;

function retentionLabel(d: number | null) {
  if (d === null) return "sunucu varsayılanı";
  if (d === 0) return "süresiz";
  return `${d} gün`;
}

export default function Sources() {
  const { role } = useAuth();
  const { confirm, toast } = useUi();
  const [q, setQ] = useState("");
  const [state, setState] = useState<"all" | "on" | "off">("all");
  const [plugin, setPlugin] = useState("");
  const plugins = useLoad(() => Api.plugins());
  const list = useLoad(
    () => Api.sources({ q: q.trim() || undefined, plugin_id: plugin || undefined, enabled: state === "all" ? undefined : state === "on" }),
    [q, state, plugin],
  );
  const [editing, setEditing] = useState<string | "new" | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const canEdit = can(role, "analyst");

  const toggle = async (s: Source) => {
    try {
      await Api.updateSource(s.id, { enabled: !s.enabled });
      list.setData((rows) => rows?.map((r) => (r.id === s.id ? { ...r, enabled: !s.enabled } : r)) ?? null);
      toast(s.enabled ? "Kaynak devre dışı bırakıldı" : "Kaynak etkinleştirildi");
    } catch (e) {
      toast(errMsg(e), "error");
    }
  };

  const runNow = async (s: Source) => {
    setRunning(s.id);
    try {
      const r = await Api.runSource(s.id);
      toast(`${s.name}: ${r.items} öğe toplandı`);
    } catch (e) {
      toast(`${s.name}: ${errMsg(e)}`, "error");
    } finally {
      setRunning(null);
      list.reload();
    }
  };

  const remove = async (s: Source) => {
    if (!(await confirm({
      title: `“${s.name}” silinsin mi?`,
      body: "Kaynak tanımı ve zamanlaması silinir. Daha önce toplanan öğeler kalır (kaynak bağlantıları boşalır).",
      confirmLabel: "Kaynağı sil", danger: true,
    }))) return;
    try {
      await Api.deleteSource(s.id);
      toast("Kaynak silindi");
      list.reload();
    } catch (e) {
      toast(errMsg(e), "error");
    }
  };

  const rows = list.data ?? [];

  return (
    <div>
      <PageHeader
        title="Kaynaklar"
        lede="Düzenli toplanan kaynaklar. Collector zamanlayıcısı her kaynağı kendi cron ifadesiyle çalıştırır ve sağlığını (son başarı, ardışık hata, yanıt süresi) kaydeder."
        actions={canEdit ? <button className="btn-primary" onClick={() => setEditing("new")}>Kaynak ekle</button> : undefined}
      />

      <div className="mb-4 flex flex-wrap items-center gap-2">
        <input id="src-q" className="field max-w-xs" placeholder="Ad ya da adreste ara" aria-label="Kaynak ara" value={q} onChange={(e) => setQ(e.target.value)} />
        <select id="src-plugin" aria-label="Plugin" className="field w-48" value={plugin} onChange={(e) => setPlugin(e.target.value)}>
          <option value="">Tüm plugin'ler</option>
          {(plugins.data ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <Segmented label="Durum" value={state} onChange={setState}
          options={[{ value: "all", label: "Tümü" }, { value: "on", label: "Etkin" }, { value: "off", label: "Kapalı" }]} />
      </div>

      {list.error && <Notice>{list.error}</Notice>}
      {list.loading && !list.data ? <Loading /> : rows.length === 0 ? (
        <Empty title={q || plugin || state !== "all" ? "Eşleşen kaynak yok" : "Henüz kaynak yok"}>
          {canEdit ? "“Kaynak ekle” ile bir RSS akışı, web sayfası, API ya da .onion adresi tanımlayın." : "Bir analist kaynak eklediğinde burada görünür."}
        </Empty>
      ) : (
        <div className="panel overflow-x-auto">
          <table className="w-full min-w-[820px] text-sm">
            <thead>
              <tr className="border-b border-osiris-line text-left text-xs text-osiris-muted">
                <th className="px-4 py-2.5 font-medium">Kaynak</th>
                <th className="px-4 py-2.5 font-medium">Plugin</th>
                <th className="px-4 py-2.5 font-medium">Zamanlama</th>
                <th className="px-4 py-2.5 font-medium">Durum</th>
                <th className="px-4 py-2.5 font-medium">Son başarı</th>
                <th className="px-4 py-2.5 text-right font-medium">Yanıt</th>
                <th className="px-4 py-2.5 text-right font-medium">Eylemler</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-osiris-line">
              {rows.map((s) => {
                const h = health(s);
                return (
                  <tr key={s.id} className="hover:bg-osiris-raise">
                    <td className="max-w-[18rem] px-4 py-2.5">
                      <button className="block max-w-full truncate text-left font-medium hover:text-osiris-accent" onClick={() => setEditing(s.id)}>{s.name}</button>
                      <div className="truncate font-mono text-xs text-osiris-faint">{s.url || "—"}</div>
                    </td>
                    <td className="px-4 py-2.5">
                      <div className="font-mono text-xs">{s.plugin_id}</div>
                      <div className="text-[11px] text-osiris-faint">{NETWORK_LABEL[s.network_type] ?? s.network_type} · öncelik {s.priority}</div>
                    </td>
                    <td className="px-4 py-2.5 font-mono text-xs text-osiris-muted">{s.schedule || "elle"}</td>
                    <td className="px-4 py-2.5"><Pill tone={h.tone}>{h.label}</Pill></td>
                    <td className="whitespace-nowrap px-4 py-2.5 text-xs text-osiris-muted" title={fmtDate(s.last_success_at)}>{fmtAgo(s.last_success_at)}</td>
                    <td className="tnum whitespace-nowrap px-4 py-2.5 text-right text-xs text-osiris-muted">{s.avg_response_ms != null ? `${s.avg_response_ms} ms` : "—"}</td>
                    <td className="whitespace-nowrap px-4 py-2.5 text-right">
                      {canEdit ? (
                        <div className="inline-flex items-center gap-3 text-xs">
                          <button className="text-osiris-accent hover:underline disabled:opacity-40" disabled={!s.enabled || running === s.id} title={s.enabled ? "" : "Kapalı kaynak çalıştırılamaz"} onClick={() => runNow(s)}>
                            {running === s.id ? "Çalışıyor" : "Çalıştır"}
                          </button>
                          <button className="text-osiris-muted hover:text-osiris-ink" onClick={() => toggle(s)}>{s.enabled ? "Kapat" : "Aç"}</button>
                          <button className="text-osiris-muted hover:text-osiris-ink" onClick={() => setEditing(s.id)}>Düzenle</button>
                          <button className="text-osiris-bad/80 hover:text-osiris-bad" onClick={() => remove(s)}>Sil</button>
                        </div>
                      ) : <button className="text-xs text-osiris-muted hover:text-osiris-ink" onClick={() => setEditing(s.id)}>Ayrıntı</button>}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      <SourceEditor
        id={editing}
        plugins={plugins.data ?? []}
        readOnly={!canEdit}
        onClose={() => setEditing(null)}
        onSaved={() => { setEditing(null); list.reload(); }}
      />
    </div>
  );
}

function SourceEditor({ id, plugins, readOnly, onClose, onSaved }: {
  id: string | "new" | null; plugins: Plugin[]; readOnly: boolean; onClose: () => void; onSaved: () => void;
}) {
  const { toast } = useUi();
  const isNew = id === "new";
  const [detail, setDetail] = useState<SourceDetail | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [pid, setPid] = useState("");
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [network, setNetwork] = useState("www");
  const [schedule, setSchedule] = useState("");
  const [priority, setPriority] = useState(5);
  const [enabled, setEnabled] = useState(true);
  const [tags, setTags] = useState("");
  const [retention, setRetention] = useState("");
  const [values, setValues] = useState<FormValues>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const plugin = plugins.find((p) => p.id === pid) ?? null;
  const urlKey = useMemo(() => URL_KEYS.find((k) => plugin?.config_schema[k]), [plugin]);

  useEffect(() => {
    setError(null); setErrors({}); setLoadErr(null); setDetail(null);
    if (!id) return;
    if (isNew) {
      setPid(""); setName(""); setUrl(""); setNetwork("www"); setSchedule(""); setPriority(5);
      setEnabled(true); setTags(""); setRetention(""); setValues({});
      return;
    }
    Api.source(id).then((d) => {
      setDetail(d);
      setPid(d.plugin_id); setName(d.name); setUrl(d.url ?? ""); setNetwork(d.network_type);
      setSchedule(d.schedule ?? ""); setPriority(d.priority); setEnabled(d.enabled);
      setTags((d.tags ?? []).join(", ")); setRetention(d.retention_days == null ? "" : String(d.retention_days));
    }).catch((e) => setLoadErr(errMsg(e)));
  }, [id, isNew]);

  // Plugin değişince formu şemadan kur (düzenlemede kayıtlı config'le doldur)
  useEffect(() => {
    if (!plugin) return;
    if (detail && detail.plugin_id === plugin.id) setValues(valuesFromConfig(plugin.config_schema, detail.config));
    else {
      setValues(initialValues(plugin.config_schema));
      if (isNew) { setNetwork(plugin.network_type); setSchedule(plugin.schedule_default || ""); }
    }
  }, [plugin, detail, isNew]);

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!plugin) return;
    if (schedule.trim() && !CRON_RE.test(schedule.trim())) return setError("Zamanlama 5 alanlı bir cron ifadesi olmalı (ör. */15 * * * *).");
    // URL kutusu plugin'in url/feed_url/endpoint alanını besler; formda o alan gizlidir
    const vals = urlKey ? { ...values, [urlKey]: url } : values;
    const { config, errors: errs } = isNew ? toConfig(plugin.config_schema, vals) : toConfigForUpdate(plugin.config_schema, vals);
    setErrors(errs);
    if (Object.keys(errs).length) return setError(urlKey && errs[urlKey] ? "Adres zorunlu." : "Formdaki hataları düzeltin.");
    const tagList = tags.split(",").map((t) => t.trim()).filter(Boolean).slice(0, 20);
    const ret = retention.trim() === "" ? null : Math.min(36500, parseInt(retention, 10));
    setBusy(true);
    setError(null);
    try {
      if (isNew) {
        await Api.createSource({ name: name.trim(), plugin_id: plugin.id, network_type: network, url: url.trim(),
          schedule: schedule.trim(), priority, enabled, tags: tagList, config, retention_days: ret });
        toast("Kaynak eklendi");
      } else if (detail) {
        await Api.updateSource(detail.id, { name: name.trim(), url: url.trim(), schedule: schedule.trim(), priority,
          enabled, tags: tagList, config, retention_days: ret });
        toast("Kaynak güncellendi");
      }
      onSaved();
    } catch (err) {
      setError(errMsg(err));
    } finally {
      setBusy(false);
    }
  };

  const href = safeHref(url);

  return (
    <Drawer open={!!id} onClose={onClose} wide title={isNew ? "Yeni kaynak" : detail?.name ?? "Kaynak"}>
      {loadErr && <Notice>{loadErr}</Notice>}
      {!isNew && !detail && !loadErr ? <Loading /> : (
        <form onSubmit={save} className="space-y-5">
          {detail && (
            <div className="rounded-md border border-osiris-line bg-osiris-sink p-3">
              <KeyValue rows={[
                ["Son çalışma", fmtDate(detail.last_crawled_at)],
                ["Son başarı", fmtDate(detail.last_success_at)],
                ["Ardışık hata", String(detail.failure_count)],
                ["Ort. yanıt", detail.avg_response_ms != null ? `${detail.avg_response_ms} ms` : "—"],
                ["Saklama", retentionLabel(detail.retention_days)],
                ["Kimlik", <span className="break-all font-mono text-xs">{detail.id}</span>],
              ]} />
            </div>
          )}
          <fieldset disabled={readOnly} className="space-y-5">
            <div className="grid gap-4 sm:grid-cols-2">
              <Label label="Plugin" required htmlFor="se-plugin">
                <select id="se-plugin" className="field" value={pid} onChange={(e) => setPid(e.target.value)} disabled={!isNew}>
                  <option value="">Seçin</option>
                  {plugins.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
                </select>
              </Label>
              <Label label="Ad" required htmlFor="se-name">
                <input id="se-name" className="field" maxLength={500} value={name} onChange={(e) => setName(e.target.value)} />
              </Label>
            </div>
            {plugin && (
              <>
                {urlKey && (
                  <Label label={`Adres (${urlKey})`} required={!!plugin.config_schema[urlKey]?.required} htmlFor="se-url"
                    hint={href ? <a className="text-osiris-accent hover:underline" href={href} target="_blank" rel="noopener noreferrer">Adresi aç</a> : undefined}>
                    <input id="se-url" className={`field font-mono ${errors[urlKey] ? "border-osiris-bad" : ""}`} maxLength={2048} value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://" />
                  </Label>
                )}
                <div className="grid gap-4 sm:grid-cols-3">
                  <Label label="Ağ türü" htmlFor="se-net">
                    <select id="se-net" className="field" value={network} onChange={(e) => setNetwork(e.target.value)} disabled={!isNew}>
                      {NETWORK_TYPES.map((n) => <option key={n} value={n}>{NETWORK_LABEL[n] ?? n}</option>)}
                    </select>
                  </Label>
                  <Label label="Öncelik" hint="1 (düşük) – 10 (yüksek)" htmlFor="se-prio">
                    <input id="se-prio" type="number" min={1} max={10} className="field tnum" value={priority} onChange={(e) => setPriority(Math.max(1, Math.min(10, Number(e.target.value) || 5)))} />
                  </Label>
                  <Label label="Saklama (gün)" hint="Boş = varsayılan (180), 0 = silinmez" htmlFor="se-ret">
                    <input id="se-ret" className="field tnum" inputMode="numeric" value={retention} onChange={(e) => setRetention(e.target.value.replace(/\D/g, ""))} />
                  </Label>
                </div>
                <div>
                  <Label label="Zamanlama (cron)" hint="dakika saat gün ay haftanın-günü · boş = yalnızca elle çalışır" htmlFor="se-cron">
                    <input id="se-cron" className="field font-mono" maxLength={100} value={schedule} onChange={(e) => setSchedule(e.target.value)} placeholder="*/15 * * * *" />
                  </Label>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {CRON_PRESETS.map((p) => (
                      <button key={p.label} type="button" onClick={() => setSchedule(p.value)}
                        className={`rounded-full border px-2.5 py-0.5 text-xs ${schedule === p.value ? "border-osiris-accent text-osiris-accent" : "border-osiris-line text-osiris-muted hover:text-osiris-ink"}`}>{p.label}</button>
                    ))}
                  </div>
                </div>
                <Label label="Etiketler" hint="Virgülle ayırın" htmlFor="se-tags">
                  <input id="se-tags" className="field" value={tags} onChange={(e) => setTags(e.target.value)} placeholder="tehdit, haber" />
                </Label>
                <Toggle id="se-enabled" checked={enabled} onChange={setEnabled} label="Etkin (zamanlayıcı çalıştırır)" />
                <div className="border-t border-osiris-line pt-4">
                  <h3 className="mb-3 text-sm font-semibold">Plugin ayarları</h3>
                  <SchemaForm schema={plugin.config_schema} values={values} onChange={setValues} errors={errors} idPrefix="se" omit={urlKey ? [urlKey] : []} />
                  {!isNew && <p className="mt-2 text-xs text-osiris-faint">Gizli alanlar (anahtar, jeton) maskeli gösterilir; değiştirmezseniz kayıtlı değer korunur.</p>}
                </div>
              </>
            )}
          </fieldset>
          {error && <Notice onClose={() => setError(null)}>{error}</Notice>}
          {!readOnly && (
            <div className="flex justify-end gap-2 border-t border-osiris-line pt-4">
              <button type="button" className="btn-ghost" onClick={onClose}>Vazgeç</button>
              <button className="btn-primary" disabled={busy || !plugin || !name.trim()}>{busy ? "Kaydediliyor" : isNew ? "Kaynağı ekle" : "Değişiklikleri kaydet"}</button>
            </div>
          )}
        </form>
      )}
    </Drawer>
  );
}
