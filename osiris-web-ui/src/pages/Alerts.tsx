// Uyarı kuralları: kayıtlı sorgular (GET/POST/DELETE /saved-queries) ve kuru deneme (POST /alerts/test).
// alert_enabled sorgular pipeline'da yeni öğelerle eşleşince e-posta/Telegram/webhook işleyicilerine gider.
import { useState } from "react";
import { Link } from "react-router-dom";
import { Api, can, QUERY_LABEL, QUERY_TYPES, type AlertMatch } from "../api";
import { useAuth } from "../auth";
import { Empty, Label, Loading, Notice, PageHeader, Pill, Segmented, Toggle, useLoad, useUi } from "../ui";
import { errMsg, fmtAgo } from "../util";

export default function Alerts() {
  const { role } = useAuth();
  const { confirm, toast } = useUi();
  const [only, setOnly] = useState<"all" | "on">("all");
  const list = useLoad(() => Api.savedQueries(only === "on" ? true : undefined), [only]);
  const [form, setForm] = useState({ name: "", query_text: "", query_type: "fts", alert_enabled: true });
  const [busy, setBusy] = useState(false);
  const [text, setText] = useState("");
  const [title, setTitle] = useState("");
  const [matches, setMatches] = useState<AlertMatch[] | null>(null);
  const [testing, setTesting] = useState(false);
  const canEdit = can(role, "analyst");

  const create = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await Api.createSavedQuery({ ...form, name: form.name.trim(), query_text: form.query_text.trim() });
      toast("Kural kaydedildi");
      setForm({ ...form, name: "", query_text: "" });
      list.reload();
    } catch (err) {
      toast(errMsg(err), "error");
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: string, name: string) => {
    if (!(await confirm({ title: `“${name}” silinsin mi?`, confirmLabel: "Sil", danger: true }))) return;
    try {
      await Api.deleteSavedQuery(id);
      list.reload();
    } catch (err) {
      toast(errMsg(err), "error");
    }
  };

  const test = async (e: React.FormEvent) => {
    e.preventDefault();
    setTesting(true);
    try {
      setMatches(await Api.testAlerts(text, title));
      list.reload();
    } catch (err) {
      toast(errMsg(err), "error");
    } finally {
      setTesting(false);
    }
  };

  const rows = list.data ?? [];

  return (
    <div>
      <PageHeader
        title="Uyarı kuralları"
        lede="Kayıtlı sorgular yeni toplanan her öğeye karşı denenir. Uyarısı açık bir kural eşleşince bildirim işleyicileri (e-posta, Telegram, webhook) tetiklenir ve kuralın son tetiklenme zamanı güncellenir."
      />
      <div className="grid gap-8 lg:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
        <section className="min-w-0">
          <div className="mb-3 flex items-center justify-between gap-3">
            <h2 className="font-semibold">Kurallar <span className="tnum text-osiris-faint">{rows.length}</span></h2>
            <Segmented label="Süzgeç" value={only} onChange={setOnly} options={[{ value: "all", label: "Tümü" }, { value: "on", label: "Uyarısı açık" }]} />
          </div>
          {list.error && <Notice>{list.error}</Notice>}
          {list.loading && !list.data ? <Loading /> : rows.length === 0 ? (
            <Empty title="Kural yok">Aşağıdan ya da Arama sayfasındaki “Uyarı olarak kaydet” ile ekleyin.</Empty>
          ) : (
            <ul className="panel divide-y divide-osiris-line">
              {rows.map((q) => (
                <li key={q.id} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate font-medium">{q.name}</span>
                      {q.alert_enabled ? <Pill tone="accent">uyarı açık</Pill> : <Pill>yalnızca kayıt</Pill>}
                    </div>
                    <div className="mt-0.5 truncate text-xs text-osiris-muted">
                      <span className="text-osiris-faint">{QUERY_LABEL[q.query_type] ?? q.query_type}</span> · <span className="font-mono">{q.query_text}</span>
                    </div>
                  </div>
                  <span className="text-xs text-osiris-faint">son tetiklenme {fmtAgo(q.last_triggered_at)}</span>
                  <div className="flex gap-3 text-xs">
                    <Link className="text-osiris-accent hover:underline" to={`/search?mode=${q.query_type === "semantic" ? "semantic" : "fts"}&q=${encodeURIComponent(q.query_text)}`}>Çalıştır</Link>
                    {canEdit && <button className="text-osiris-bad/80 hover:text-osiris-bad" onClick={() => remove(q.id, q.name)}>Sil</button>}
                  </div>
                </li>
              ))}
            </ul>
          )}

          {canEdit && (
            <form onSubmit={create} className="panel mt-6 space-y-4 p-5">
              <h2 className="font-semibold">Yeni kural</h2>
              <div className="grid gap-4 sm:grid-cols-2">
                <Label label="Ad" required htmlFor="aq-name"><input id="aq-name" className="field" maxLength={200} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="Kurum adı geçen sızıntılar" /></Label>
                <Label label="Tür" htmlFor="aq-type">
                  <select id="aq-type" className="field" value={form.query_type} onChange={(e) => setForm({ ...form, query_type: e.target.value })}>
                    {QUERY_TYPES.map((t) => <option key={t} value={t}>{QUERY_LABEL[t]}</option>)}
                  </select>
                </Label>
              </div>
              <Label label="Sorgu" required hint="Eşleşme öğe başlığı ve temizlenmiş metninde alt metin olarak aranır (büyük/küçük harf duyarsız)." htmlFor="aq-text">
                <input id="aq-text" className="field font-mono" maxLength={500} value={form.query_text} onChange={(e) => setForm({ ...form, query_text: e.target.value })} placeholder="ornekkurum.com.tr" />
              </Label>
              <div className="flex flex-wrap items-center justify-between gap-3">
                <Toggle id="aq-alert" checked={form.alert_enabled} onChange={(v) => setForm({ ...form, alert_enabled: v })} label="Eşleşince uyarı gönder" />
                <button className="btn-primary" disabled={busy || !form.name.trim() || !form.query_text.trim()}>Kuralı kaydet</button>
              </div>
            </form>
          )}
        </section>

        <section className="min-w-0">
          <h2 className="mb-3 font-semibold">Kuralları dene</h2>
          {canEdit ? (
            <form onSubmit={test} className="panel space-y-3 p-5">
              <p className="text-sm text-osiris-muted">Bir metni uyarısı açık kurallara karşı deneyin. Öğe kaydedilmez; eşleşen kuralların son tetiklenme zamanı güncellenir.</p>
              <Label label="Başlık" htmlFor="at-title"><input id="at-title" className="field" maxLength={2000} value={title} onChange={(e) => setTitle(e.target.value)} /></Label>
              <Label label="Metin" required htmlFor="at-text">
                <textarea id="at-text" className="field min-h-[9rem]" maxLength={50000} value={text} onChange={(e) => setText(e.target.value)} placeholder="Bir forum gönderisini ya da haber metnini yapıştırın" />
              </Label>
              <button className="btn-primary w-full" disabled={testing || !text.trim()}>{testing ? "Deneniyor" : "Dene"}</button>
              {matches && (
                <div className="border-t border-osiris-line pt-3">
                  {matches.length === 0 ? <p className="text-sm text-osiris-muted">Hiçbir kural eşleşmedi.</p> : (
                    <ul className="space-y-1.5">
                      {matches.map((m, i) => (
                        <li key={i} className="flex items-center justify-between gap-3 text-sm">
                          <span className="truncate"><span className="text-osiris-warn">●</span> {m.query_name}</span>
                          <span className="truncate font-mono text-xs text-osiris-muted">“{m.matched}”</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              )}
            </form>
          ) : <Notice kind="info">Kuralları denemek için analist rolü gerekir.</Notice>}
          <div className="mt-6 space-y-2 text-sm text-osiris-muted">
            <p className="font-medium text-osiris-ink">Anomali tespiti</p>
            <p>Kaynak başına öğe hacmi kayan z-skor tabanıyla izlenir (eşik 3.0, ısınma dönemi sonrası). Ani sıçramalar aynı bildirim kanalından gider; ayrı bir kural gerekmez.</p>
          </div>
        </section>
      </div>
    </div>
  );
}
