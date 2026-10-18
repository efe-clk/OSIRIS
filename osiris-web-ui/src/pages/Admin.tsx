// Erişim ve gizlilik (admin): kullanıcı API anahtarları (GET/POST/DELETE /auth/keys),
// kısa ömürlü JWT (POST /auth/token), KVKK varlık silme (POST /entities/erase), saklama temizliği.
import { useState } from "react";
import { Api, can, ENTITY_LABEL, ENTITY_TYPES, type Role } from "../api";
import { useAuth } from "../auth";
import { Empty, Label, Loading, Notice, PageHeader, Pill, Section, Toggle, useLoad, useUi } from "../ui";
import { copyText, errMsg, fmtAgo, fmtDate } from "../util";

const ROLE_LABEL: Record<Role, string> = { viewer: "İzleyici", analyst: "Analist", admin: "Yönetici" };
const ROLE_HINT: Record<Role, string> = {
  viewer: "Okur ve arar; toplama başlatamaz.",
  analyst: "Toplama, kaynak, kural, graf ve rapor işlemleri.",
  admin: "Ek olarak anahtar yönetimi ve KVKK silme.",
};

function CopyBox({ value, label }: { value: string; label: string }) {
  const { toast } = useUi();
  return (
    <div className="flex gap-2">
      <input readOnly aria-label={label} value={value} className="field font-mono text-xs" onFocus={(e) => e.currentTarget.select()} />
      <button type="button" className="btn-ghost shrink-0" onClick={async () => toast((await copyText(value)) ? "Kopyalandı" : "Kopyalanamadı; kutuyu seçip elle kopyalayın", "ok")}>Kopyala</button>
    </div>
  );
}

export default function Admin() {
  const { role } = useAuth();
  const { confirm, toast } = useUi();
  const keys = useLoad(() => (can(role, "admin") ? Api.keys() : Promise.resolve([])), [role]);
  const [nk, setNk] = useState<{ username: string; role: Role; name: string }>({ username: "", role: "analyst", name: "" });
  const [minted, setMinted] = useState<{ api_key: string; role: Role; username: string } | null>(null);
  const [showRevoked, setShowRevoked] = useState(false);
  const [tk, setTk] = useState<{ api_key: string; role: Role }>({ api_key: "", role: "viewer" });
  const [token, setToken] = useState<{ access_token: string; expires_in: number } | null>(null);
  const [er, setEr] = useState({ entity_type: "email", value: "", delete_items: true });
  const [erResult, setErResult] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  if (!can(role, "admin")) return <Notice kind="info">Bu sayfa yalnızca yönetici rolüne açıktır.</Notice>;

  const createKey = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy("key");
    try {
      const r = await Api.createKey(nk.username.trim(), nk.role, nk.name.trim());
      setMinted({ api_key: r.api_key, role: r.role, username: nk.username.trim() });
      setNk({ ...nk, username: "", name: "" });
      keys.reload();
    } catch (err) {
      toast(errMsg(err), "error");
    } finally {
      setBusy(null);
    }
  };

  const revoke = async (id: string, label: string) => {
    if (!(await confirm({ title: "Anahtar iptal edilsin mi?", body: `${label} anahtarı hemen geçersiz olur. Kayıt denetim izi için tutulur.`, confirmLabel: "İptal et", danger: true }))) return;
    try {
      await Api.revokeKey(id);
      toast("Anahtar iptal edildi");
      keys.reload();
    } catch (err) {
      toast(errMsg(err), "error");
    }
  };

  const mintToken = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy("jwt");
    setToken(null);
    try {
      setToken(await Api.token(tk.api_key.trim(), tk.role));
      setTk({ ...tk, api_key: "" });
    } catch (err) {
      toast(errMsg(err).includes("503") || errMsg(err).includes("JWT") ? "JWT kapalı: sunucuda OSIRIS_JWT_SECRET tanımlı değil." : errMsg(err), "error");
    } finally {
      setBusy(null);
    }
  };

  const erase = async (e: React.FormEvent) => {
    e.preventDefault();
    const v = er.value.trim();
    if (!(await confirm({
      title: "Kişisel veri kalıcı olarak silinsin mi?",
      body: (
        <>
          <p><b>{ENTITY_LABEL[er.entity_type as keyof typeof ENTITY_LABEL]}</b> türündeki <span className="font-mono">{v}</span> değeri (büyük/küçük harf duyarsız) silinecek.</p>
          <p className="mt-2">{er.delete_items ? "Bu değeri anan tüm öğeler de silinir." : "Öğeler kalır, yalnızca varlık kaydı ve graf kenarları silinir."} Geri alınamaz.</p>
        </>
      ),
      confirmLabel: "Kalıcı olarak sil", danger: true,
    }))) return;
    setBusy("erase");
    try {
      const r = await Api.eraseEntity(er.entity_type, v, er.delete_items);
      setErResult(`${r.items_deleted} öğe, ${r.entities_deleted} varlık ve ${r.edges_deleted} graf kenarı silindi. Denetim kaydına yalnızca değerin SHA-256 özeti yazıldı.`);
      setEr({ ...er, value: "" });
    } catch (err) {
      toast(errMsg(err), "error");
    } finally {
      setBusy(null);
    }
  };

  const retention = async () => {
    if (!(await confirm({
      title: "Saklama temizliği şimdi çalıştırılsın mı?",
      body: "Kaynak başına saklama süresi (yoksa sunucu varsayılanı, genelde 180 gün) dolan öğeler ve bağsız kalan varlıklar kalıcı olarak silinir.",
      confirmLabel: "Temizliği çalıştır", danger: true,
    }))) return;
    setBusy("ret");
    try {
      const r = await Api.runRetention();
      toast(`${r.items_deleted} öğe ve ${r.entities_deleted} varlık silindi${r.default_days !== undefined ? ` (varsayılan ${r.default_days} gün)` : ""}`);
    } catch (err) {
      toast(errMsg(err), "error");
    } finally {
      setBusy(null);
    }
  };

  const rows = (keys.data ?? []).filter((k) => showRevoked || !k.revoked);

  return (
    <div className="space-y-10">
      <PageHeader title="Erişim ve gizlilik" lede="Kullanıcı anahtarları, otomasyon için kısa ömürlü jetonlar ve KVKK kapsamında kişisel veri silme." />

      <Section title="API anahtarları" aside={<Toggle id="ad-rev" checked={showRevoked} onChange={setShowRevoked} label="İptal edilenleri göster" />}>
        {minted && (
          <Notice kind="ok" onClose={() => setMinted(null)}>
            <p className="mb-2"><b>{minted.username}</b> için {ROLE_LABEL[minted.role].toLowerCase()} anahtarı oluşturuldu. Bu değer <b>bir daha gösterilmez</b>; şimdi kopyalayıp güvenli bir yere kaydedin.</p>
            <CopyBox value={minted.api_key} label="Yeni API anahtarı" />
          </Notice>
        )}
        {keys.error && <Notice>{keys.error}</Notice>}
        {keys.loading && !keys.data ? <Loading /> : rows.length === 0 ? (
          <Empty title="Kullanıcı anahtarı yok">Operatör anahtarı (.env içindeki OSIRIS_API_KEY) burada listelenmez.</Empty>
        ) : (
          <div className="panel overflow-x-auto">
            <table className="w-full min-w-[720px] text-sm">
              <thead>
                <tr className="border-b border-osiris-line text-left text-xs text-osiris-muted">
                  <th className="px-4 py-2.5 font-medium">Kullanıcı</th>
                  <th className="px-4 py-2.5 font-medium">Rol</th>
                  <th className="px-4 py-2.5 font-medium">Önek</th>
                  <th className="px-4 py-2.5 font-medium">Ad</th>
                  <th className="px-4 py-2.5 font-medium">Oluşturuldu</th>
                  <th className="px-4 py-2.5 font-medium">Son kullanım</th>
                  <th className="px-4 py-2.5 text-right font-medium" />
                </tr>
              </thead>
              <tbody className="divide-y divide-osiris-line">
                {rows.map((k) => (
                  <tr key={k.id} className={k.revoked ? "opacity-50" : ""}>
                    <td className="px-4 py-2.5 font-medium">{k.username}</td>
                    <td className="px-4 py-2.5"><Pill tone={k.role === "admin" ? "bad" : k.role === "analyst" ? "accent" : "idle"}>{ROLE_LABEL[k.role]}</Pill></td>
                    <td className="px-4 py-2.5 font-mono text-xs">{k.key_prefix}…</td>
                    <td className="px-4 py-2.5 text-osiris-muted">{k.name || "—"}</td>
                    <td className="whitespace-nowrap px-4 py-2.5 text-xs text-osiris-muted">{fmtDate(k.created_at)}</td>
                    <td className="whitespace-nowrap px-4 py-2.5 text-xs text-osiris-muted">{fmtAgo(k.last_used_at)}</td>
                    <td className="px-4 py-2.5 text-right">
                      {k.revoked ? <span className="text-xs text-osiris-faint">iptal</span> : (
                        <button className="text-xs text-osiris-bad/80 hover:text-osiris-bad" onClick={() => revoke(k.id, `${k.username} (${k.key_prefix}…)`)}>İptal et</button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <form onSubmit={createKey} className="panel mt-4 p-5">
          <h3 className="mb-3 font-semibold">Yeni anahtar</h3>
          <div className="grid gap-4 sm:grid-cols-[1fr_12rem_1fr_auto] sm:items-end">
            <Label label="Kullanıcı adı" required htmlFor="nk-user"><input id="nk-user" className="field" maxLength={100} value={nk.username} onChange={(e) => setNk({ ...nk, username: e.target.value })} placeholder="ayse" /></Label>
            <Label label="Rol" htmlFor="nk-role">
              <select id="nk-role" className="field" value={nk.role} onChange={(e) => setNk({ ...nk, role: e.target.value as Role })}>
                {(["viewer", "analyst", "admin"] as Role[]).map((r) => <option key={r} value={r}>{ROLE_LABEL[r]}</option>)}
              </select>
            </Label>
            <Label label="Anahtar adı" htmlFor="nk-name"><input id="nk-name" className="field" maxLength={100} value={nk.name} onChange={(e) => setNk({ ...nk, name: e.target.value })} placeholder="dizüstü, n8n…" /></Label>
            <button className="btn-primary" disabled={busy === "key" || !nk.username.trim()}>Oluştur</button>
          </div>
          <p className="mt-2 text-xs text-osiris-faint">{ROLE_HINT[nk.role]} Sunucuda yalnızca SHA-256 özeti saklanır.</p>
        </form>
      </Section>

      <Section title="Kısa ömürlü jeton (JWT)">
        <form onSubmit={mintToken} className="panel space-y-4 p-5">
          <p className="text-sm text-osiris-muted">Üçüncü taraf araçlara kalıcı anahtar yerine 1 saatlik Bearer jeton verin. Jeton üretmek için operatör anahtarı gerekir; anahtar yalnızca bu istekte kullanılır, saklanmaz.</p>
          <div className="grid gap-4 sm:grid-cols-[1fr_12rem_auto] sm:items-end">
            <Label label="Operatör anahtarı" required htmlFor="tk-key"><input id="tk-key" type="password" autoComplete="off" className="field font-mono" value={tk.api_key} onChange={(e) => setTk({ ...tk, api_key: e.target.value })} /></Label>
            <Label label="Jeton rolü" htmlFor="tk-role">
              <select id="tk-role" className="field" value={tk.role} onChange={(e) => setTk({ ...tk, role: e.target.value as Role })}>
                {(["viewer", "analyst", "admin"] as Role[]).map((r) => <option key={r} value={r}>{ROLE_LABEL[r]}</option>)}
              </select>
            </Label>
            <button className="btn-primary" disabled={busy === "jwt" || !tk.api_key.trim()}>Jeton üret</button>
          </div>
          {token && (
            <div className="space-y-2 border-t border-osiris-line pt-4">
              <p className="text-sm">Jeton {Math.round(token.expires_in / 60)} dakika geçerli. <span className="text-osiris-muted">Başlık: <span className="font-mono">Authorization: Bearer …</span></span></p>
              <CopyBox value={token.access_token} label="JWT" />
            </div>
          )}
        </form>
      </Section>

      <Section title="KVKK: kişisel veri silme">
        <div className="grid gap-6 lg:grid-cols-2">
          <form onSubmit={erase} className="panel space-y-4 p-5">
            <h3 className="font-semibold">İlgili kişinin verisini sil</h3>
            <p className="text-sm text-osiris-muted">Silme talebinde bulunan kişinin e-posta, telefon, kullanıcı adı gibi değerini girin. Eşleşme tür + büyük/küçük harf duyarsız tam değerdir.</p>
            <div className="grid gap-4 sm:grid-cols-[11rem_1fr]">
              <Label label="Tür" htmlFor="er-type">
                <select id="er-type" className="field" value={er.entity_type} onChange={(e) => setEr({ ...er, entity_type: e.target.value })}>
                  {ENTITY_TYPES.map((t) => <option key={t} value={t}>{ENTITY_LABEL[t]}</option>)}
                </select>
              </Label>
              <Label label="Değer" required htmlFor="er-val"><input id="er-val" className="field font-mono" maxLength={500} value={er.value} onChange={(e) => setEr({ ...er, value: e.target.value })} placeholder="ali@example.com" /></Label>
            </div>
            <Toggle id="er-items" checked={er.delete_items} onChange={(v) => setEr({ ...er, delete_items: v })} label="Bu değeri anan öğeleri de sil" />
            {erResult && <Notice kind="ok" onClose={() => setErResult(null)}>{erResult}</Notice>}
            <button className="btn-danger w-full" disabled={busy === "erase" || !er.value.trim()}>Kalıcı olarak sil</button>
          </form>
          <div className="panel space-y-4 p-5">
            <h3 className="font-semibold">Saklama süresi temizliği</h3>
            <p className="text-sm text-osiris-muted">Pipeline işçisi bunu saatte bir kendiliğinden yapar. İşçi çalışmıyorsa ya da hemen uygulamak istiyorsanız buradan tetikleyin. Kaynak başına süreyi Kaynaklar sayfasından ayarlayın.</p>
            <button className="btn-danger" disabled={busy === "ret"} onClick={retention}>{busy === "ret" ? "Çalışıyor" : "Temizliği şimdi çalıştır"}</button>
          </div>
        </div>
      </Section>
    </div>
  );
}
