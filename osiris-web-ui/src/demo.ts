// Demo modu: /api/* isteklerini tarayıcıda yanıtlayan bellek-içi sahte API.
// Yalnızca `VITE_DEMO=1 npm run build` ile derlenen pakete girer (main.tsx dinamik import).
// Veriler örnek veridir (example.* alan adları, belgeleme IP blokları); gerçek sistemle ilgisi yoktur.
import { session } from "./api";
import demoPlugins from "./demoPlugins.json";
import demoSites from "./demoSites.json";

type J = Record<string, unknown>;

const now = Date.now();
const ago = (min: number) => new Date(now - min * 60_000).toISOString();
let seq = 1000;
const uid = () => {
  seq++;
  const h = (seq * 2654435761 >>> 0).toString(16).padStart(8, "0");
  return `${h}-1d2e-4f3a-9b8c-${seq.toString(16).padStart(12, "0")}`;
};

// ------------------------------------------------------------------ örnek veri

interface DItem {
  id: string; source_id: string | null; title: string; url: string; language: string;
  cleaned_content: string; collected_at: string; published_at: string | null; tags: string[];
  entities: { type: string; value: string; mention_count: number }[];
}

const SOURCES: J[] = [
  { name: "CISA güvenlik bildirimleri", plugin_id: "rss", network_type: "rss", url: "https://feeds.example.org/cisa/advisories.xml", schedule: "*/15 * * * *", priority: 8, enabled: true, tags: ["tehdit"], failure_count: 0, avg_response_ms: 412, last: 6, retention_days: null, config: { feed_url: "https://feeds.example.org/cisa/advisories.xml", max_items: 50 } },
  { name: "Siber haber akışı (TR)", plugin_id: "rss", network_type: "rss", url: "https://haber.example.com.tr/siber/rss", schedule: "0 * * * *", priority: 5, enabled: true, tags: ["haber"], failure_count: 0, avg_response_ms: 690, last: 22, retention_days: 90, config: { feed_url: "https://haber.example.com.tr/siber/rss" } },
  { name: "Sızıntı forumu (onion)", plugin_id: "tor", network_type: "tor", url: "http://exampleleakforumxyz.onion/latest", schedule: "0 */6 * * *", priority: 9, enabled: true, tags: ["darknet", "sızıntı"], failure_count: 2, avg_response_ms: 4810, last: 95, retention_days: 30, config: { url: "http://exampleleakforumxyz.onion/latest" } },
  { name: "Shodan · açık RDP (TR)", plugin_id: "shodan", network_type: "api", url: "", schedule: "0 3 * * *", priority: 6, enabled: true, tags: ["altyapı"], failure_count: 0, avg_response_ms: 1220, last: 480, retention_days: null, config: { api_key: "********", query: "port:3389 country:TR" } },
  { name: "ornekkurum.com.tr DNS/WHOIS", plugin_id: "dns-whois", network_type: "api", url: "", schedule: "0 3 * * *", priority: 4, enabled: true, tags: ["hedef"], failure_count: 0, avg_response_ms: 230, last: 470, retention_days: 0, config: { domain: "ornekkurum.com.tr" } },
  { name: "Durum sayfası kazıyıcı", plugin_id: "web-scraper", network_type: "www", url: "https://status.example.net/", schedule: "*/30 * * * *", priority: 3, enabled: false, tags: [], failure_count: 5, avg_response_ms: null, last: 2900, retention_days: null, config: { url: "https://status.example.net/", depth: 1 } },
  { name: "IRC #osint-tr", plugin_id: "irc", network_type: "irc", url: "", schedule: "", priority: 2, enabled: true, tags: ["sohbet"], failure_count: 0, avg_response_ms: 3100, last: null, retention_days: 30, config: { server: "irc.example.net", channels: ["#osint-tr"] } },
].map((s) => ({
  id: uid(), ...s,
  last_crawled_at: s.last === null ? null : ago(s.last as number),
  last_success_at: s.last === null ? null : ago((s.last as number) + (s.failure_count as number) * 360),
  created_at: ago(60 * 24 * 20), updated_at: ago(60 * 24 * 2),
}));

const sid = (i: number) => SOURCES[i].id as string;

const ITEMS: DItem[] = [
  { source_id: sid(0), title: "Kritik: VPN ağ geçidinde kimlik doğrulama atlatma (CVE-2026-31337)", url: "https://feeds.example.org/cisa/aa26-288a", language: "en", tags: ["vulnerability", "threat"], min: 8,
    text: "CISA, VPN ağ geçitlerinde kimlik doğrulamanın atlatılmasına izin veren CVE-2026-31337 açığının aktif olarak istismar edildiğini bildirdi. Saldırganlar 203.0.113.45 ve 198.51.100.23 adreslerinden tarama yapıyor. Komuta sunucusu update-check.example.net alan adını kullanıyor. Yamalar uygulanmalı; günlüklerde bu göstergeler aranmalı.",
    ents: [["cve", "CVE-2026-31337", 2], ["ip", "203.0.113.45", 1], ["ip", "198.51.100.23", 1], ["domain", "update-check.example.net", 1], ["org", "CISA", 1]] },
  { source_id: sid(2), title: "[SATIŞ] ornekkurum.com.tr müşteri veritabanı 2026", url: "http://exampleleakforumxyz.onion/t/88213", language: "tr", tags: ["breach", "darknet"], min: 97,
    text: "ornekkurum.com.tr müşteri tablosu satılık. 412.000 kayıt: ad, e-posta, telefon. Örnek satırlar ekte. İletişim: darkseller_77 (forum DM) ya da xmr ile ödeme. İrtibat e-postası destek@ornekkurum.com.tr adresinin sızdırılmış parola listesiyle birlikte geliyor. Bitcoin adresi: bc1qexampleexampleexampleexample0x9k2.",
    ents: [["domain", "ornekkurum.com.tr", 3], ["email", "destek@ornekkurum.com.tr", 1], ["username", "darkseller_77", 2], ["crypto_address", "bc1qexampleexampleexampleexample0x9k2", 1], ["org", "Örnek Kurum", 1]] },
  { source_id: sid(1), title: "Belediyelere yönelik oltalama kampanyası genişliyor", url: "https://haber.example.com.tr/siber/oltalama-belediye", language: "tr", tags: ["phishing"], min: 25,
    text: "Ankara ve İzmir'deki belediye personeline 'e-Devlet şifre yenileme' başlıklı oltalama e-postaları gönderiliyor. Sahte sayfa edevlet-dogrula.example.com adresinde barındırılıyor ve 198.51.100.77 IP'sine çözülüyor. Gönderici adres guvenlik@edevlet-dogrula.example.com.",
    ents: [["location", "Ankara", 1], ["location", "İzmir", 1], ["domain", "edevlet-dogrula.example.com", 2], ["ip", "198.51.100.77", 1], ["email", "guvenlik@edevlet-dogrula.example.com", 1]] },
  { source_id: sid(3), title: "203.0.113.10 · 3389/tcp · Microsoft Terminal Services", url: "https://www.shodan.io/host/203.0.113.10", language: "en", tags: ["infra"], min: 480,
    text: "{'ip_str': '203.0.113.10', 'port': 3389, 'org': 'Example Hosting', 'location': {'city': 'Istanbul', 'country_code': 'TR'}, 'hostnames': ['rdp01.ornekkurum.com.tr']}",
    ents: [["ip", "203.0.113.10", 1], ["domain", "rdp01.ornekkurum.com.tr", 1], ["location", "Istanbul", 1], ["org", "Example Hosting", 1]] },
  { source_id: sid(4), title: "ornekkurum.com.tr DNS kayıtları", url: "dns://ornekkurum.com.tr", language: "en", tags: [], min: 470,
    text: "A 203.0.113.80 · MX mail.ornekkurum.com.tr · NS ns1.example-dns.net, ns2.example-dns.net · TXT v=spf1 include:_spf.example.net ~all",
    ents: [["domain", "ornekkurum.com.tr", 1], ["ip", "203.0.113.80", 1], ["domain", "mail.ornekkurum.com.tr", 1], ["domain", "ns1.example-dns.net", 1]] },
  { source_id: sid(0), title: "Fidye yazılımı grubu sağlık sektörünü hedef alıyor", url: "https://feeds.example.org/cisa/aa26-280b", language: "en", tags: ["threat"], min: 300,
    text: "A ransomware affiliate is exploiting CVE-2026-20871 in managed file transfer appliances. Exfiltration traffic was observed to 198.51.100.140. Ransom notes reference contact address recovery-team@example.org.",
    ents: [["cve", "CVE-2026-20871", 1], ["ip", "198.51.100.140", 1], ["email", "recovery-team@example.org", 1]] },
  { source_id: sid(2), title: "darkseller_77 yeni ilan: kurumsal VPN erişimi", url: "http://exampleleakforumxyz.onion/t/88301", language: "tr", tags: ["darknet"], min: 180,
    text: "darkseller_77 kullanıcısı Türkiye merkezli bir lojistik firmasının VPN erişimini satışa çıkardı. Kanıt olarak vpn.lojistik-ornek.example adresinin oturum ekranı paylaşıldı. Ödeme yalnızca Monero.",
    ents: [["username", "darkseller_77", 1], ["domain", "vpn.lojistik-ornek.example", 1], ["location", "Türkiye", 1]] },
  { source_id: null, title: "johndoe_dev @ GitHub", url: "https://github.com/johndoe_dev", language: "tr", tags: ["username", "dev"], min: 40,
    text: "Profil bulundu: GitHub — https://github.com/johndoe_dev", ents: [["username", "johndoe_dev", 1]] },
  { source_id: sid(1), title: "Yeni kimlik avı kiti Telegram üzerinden dağıtılıyor", url: "https://haber.example.com.tr/siber/phishing-kit", language: "tr", tags: ["phishing"], min: 900,
    text: "Araştırmacılar, Türk bankalarını taklit eden bir kimlik avı kitinin Telegram kanallarında satıldığını tespit etti. Kit, panel olarak panel.kit-ornek.example kullanıyor ve +90 555 010 20 30 numaralı hat üzerinden destek veriyor.",
    ents: [["domain", "panel.kit-ornek.example", 1], ["phone", "+90 555 010 20 30", 1], ["org", "Telegram", 1]] },
].map((x) => ({
  id: uid(), source_id: x.source_id, title: x.title, url: x.url, language: x.language,
  cleaned_content: x.text, collected_at: ago(x.min), published_at: ago(x.min + 30), tags: x.tags,
  entities: x.ents.map(([type, value, n]) => ({ type: type as string, value: value as string, mention_count: n as number })),
}));

const EDGES: { source: string; target: string; relation_type: string }[] = [
  ["ornekkurum.com.tr", "203.0.113.80", "resolves"], ["ornekkurum.com.tr", "mail.ornekkurum.com.tr", "mx"],
  ["ornekkurum.com.tr", "rdp01.ornekkurum.com.tr", "subdomain"], ["rdp01.ornekkurum.com.tr", "203.0.113.10", "resolves"],
  ["ornekkurum.com.tr", "ns1.example-dns.net", "ns"], ["ornekkurum.com.tr", "ns2.example-dns.net", "ns"],
  ["darkseller_77", "ornekkurum.com.tr", "sells_data_of"], ["darkseller_77", "vpn.lojistik-ornek.example", "sells_access_to"],
  ["darkseller_77", "bc1qexampleexampleexampleexample0x9k2", "owns"], ["destek@ornekkurum.com.tr", "ornekkurum.com.tr", "member_of"],
  ["edevlet-dogrula.example.com", "198.51.100.77", "resolves"], ["guvenlik@edevlet-dogrula.example.com", "edevlet-dogrula.example.com", "member_of"],
  ["update-check.example.net", "203.0.113.45", "resolves"], ["CVE-2026-31337", "203.0.113.45", "exploited_from"],
  ["CVE-2026-31337", "198.51.100.23", "exploited_from"], ["CVE-2026-20871", "198.51.100.140", "exfil_to"],
  ["johndoe_dev", "GitHub", "has_account"], ["johndoe_dev", "GitLab", "has_account"], ["johndoe_dev", "Reddit", "has_account"],
  ["johndoe_dev", "Keybase", "has_account"], ["johndoe_dev", "Docker Hub", "has_account"], ["johndoe_dev", "Telegram", "has_account"],
  ["panel.kit-ornek.example", "Telegram", "distributed_via"], ["panel.kit-ornek.example", "198.51.100.77", "resolves"],
].map(([source, target, relation_type]) => ({ source, target, relation_type }));

const QUERIES: J[] = [
  { name: "Kurum alan adı", query_text: "ornekkurum.com.tr", query_type: "fts", alert_enabled: true, last_triggered_at: ago(97) },
  { name: "Satıcı takibi: darkseller_77", query_text: "darkseller_77", query_type: "entity", alert_enabled: true, last_triggered_at: ago(180) },
  { name: "Kritik VPN açıkları", query_text: "VPN kimlik doğrulama atlatma", query_type: "semantic", alert_enabled: true, last_triggered_at: ago(8) },
  { name: "e-Devlet oltalama", query_text: "e-Devlet", query_type: "fts", alert_enabled: false, last_triggered_at: null },
].map((q) => ({ id: uid(), ...q }));

const KEYS: J[] = [
  { username: "ayse", role: "analyst", key_prefix: "osiris_4f2a", name: "dizüstü", revoked: false, created_at: ago(60 * 24 * 14), last_used_at: ago(12) },
  { username: "n8n", role: "analyst", key_prefix: "osiris_9c1d", name: "otomasyon", revoked: false, created_at: ago(60 * 24 * 30), last_used_at: ago(15) },
  { username: "mehmet", role: "viewer", key_prefix: "osiris_77be", name: "yönetim paneli", revoked: false, created_at: ago(60 * 24 * 6), last_used_at: ago(60 * 26) },
  { username: "eski-stajyer", role: "analyst", key_prefix: "osiris_0a31", name: "", revoked: true, created_at: ago(60 * 24 * 90), last_used_at: ago(60 * 24 * 40) },
].map((k) => ({ id: uid(), ...k }));

// ------------------------------------------------------------------ yardımcılar

function entityIndex() {
  const m = new Map<string, { type: string; value: string; mentions: number; last_seen: string }>();
  for (const it of ITEMS) for (const e of it.entities) {
    const k = `${e.type}:${e.value.toLowerCase()}`;
    const cur = m.get(k);
    if (cur) { cur.mentions += 1; if (it.collected_at > cur.last_seen) cur.last_seen = it.collected_at; }
    else m.set(k, { type: e.type, value: e.value, mentions: 1, last_seen: it.collected_at });
  }
  return [...m.values()].sort((a, b) => b.mentions - a.mentions || a.value.localeCompare(b.value));
}

function summary(it: DItem, n = 2000) {
  return { id: it.id, title: it.title, url: it.url, cleaned_content: it.cleaned_content.slice(0, n), collected_at: it.collected_at };
}

function graphState() {
  const nodes = new Set<string>();
  const adj = new Map<string, Set<string>>();
  for (const e of EDGES) {
    nodes.add(e.source); nodes.add(e.target);
    if (!adj.has(e.source)) adj.set(e.source, new Set());
    if (!adj.has(e.target)) adj.set(e.target, new Set());
    adj.get(e.source)!.add(e.target); adj.get(e.target)!.add(e.source);
  }
  return { nodes: [...nodes], adj };
}

function communities(nodes: string[], adj: Map<string, Set<string>>) {
  // Demo: bağlı bileşenler (sunucu CNM modülerlik kullanır)
  const seen = new Set<string>();
  const out: string[][] = [];
  for (const n of nodes) {
    if (seen.has(n)) continue;
    const comp: string[] = [];
    const st = [n];
    seen.add(n);
    while (st.length) {
      const x = st.pop()!;
      comp.push(x);
      for (const y of adj.get(x) ?? []) if (!seen.has(y)) { seen.add(y); st.push(y); }
    }
    out.push(comp.sort());
  }
  return out.sort((a, b) => b.length - a.length);
}

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));

function err(status: number, detail: string) {
  return new Response(JSON.stringify({ detail }), { status, headers: { "Content-Type": "application/json" } });
}
function ok(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function previewItems(pluginId: string, config: J) {
  const host = String(config.url ?? config.feed_url ?? config.endpoint ?? config.domain ?? config.homeserver ?? config.server ?? "kaynak.example");
  const base = host.replace(/^https?:\/\//, "").split("/")[0];
  const titles = [
    "Yeni güvenlik bülteni yayımlandı", "Kimlik bilgisi sızıntısı iddiası", "Açık RDP sunucuları raporu",
    "Kötü amaçlı alan adları listesi güncellendi", "Fidye yazılımı kurbanları sızıntı sitesinde",
  ];
  return titles.slice(0, pluginId === "dns-whois" ? 1 : 5).map((t, i) => ({
    url: `https://${base}/kayit/${1000 + i}`, title: pluginId === "dns-whois" ? `${base} DNS kayıtları` : t,
    snippet: pluginId === "dns-whois" ? `A 203.0.113.${80 + i} · MX mail.${base} · NS ns1.example-dns.net` : `${t}. Örnek içerik: göstergeler 198.51.100.${10 + i}, ${base}.`,
    published_at: ago(i * 30), metadata: { plugin: pluginId }, tags: [],
  }));
}

function hashOf(s: string) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return h >>> 0;
}

// ------------------------------------------------------------------ yönlendirici

async function route(method: string, path: string, query: URLSearchParams, body: J): Promise<Response> {
  const seg = path.split("/").filter(Boolean);
  const isUuid = (s?: string) => !!s && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s);

  if (path === "/health") return ok({ status: "ok", service: "osiris-api" });
  if (path === "/plugins") return ok(demoPlugins);
  if (path === "/stats") {
    return ok({
      sources: SOURCES.length, sources_enabled: SOURCES.filter((s) => s.enabled).length, items: ITEMS.length,
      entities: entityIndex().length, edges: EDGES.length, saved_queries: QUERIES.length,
      alerts: QUERIES.filter((q) => q.alert_enabled).length,
      latest_item_at: [...ITEMS].sort((a, b) => b.collected_at.localeCompare(a.collected_at))[0]?.collected_at ?? null,
    });
  }

  // auth
  if (path === "/auth/keys" && method === "GET") return ok(KEYS);
  if (path === "/auth/keys" && method === "POST") {
    const raw = `osiris_${Math.random().toString(36).slice(2)}${Math.random().toString(36).slice(2)}`;
    const row = { id: uid(), username: String(body.username), role: body.role, key_prefix: raw.slice(0, 11), name: String(body.name ?? ""), revoked: false, created_at: new Date().toISOString(), last_used_at: null };
    KEYS.unshift(row);
    return ok({ id: row.id, api_key: raw, role: body.role });
  }
  if (seg[0] === "auth" && seg[1] === "keys" && method === "DELETE") {
    const k = KEYS.find((x) => x.id === seg[2]);
    if (!k) return err(404, "Anahtar bulunamadı");
    k.revoked = true;
    return ok({ status: "revoked" });
  }
  if (path === "/auth/token") {
    return ok({ access_token: `eyJhbGciOiJIUzI1NiJ9.${btoa(JSON.stringify({ sub: "demo", role: body.role }))}.demo-imza`, token_type: "bearer", expires_in: 3600 });
  }

  // öğeler
  if (path === "/items/recent") {
    const limit = Math.min(50, Number(query.get("limit") ?? 20)), offset = Number(query.get("offset") ?? 0);
    return ok([...ITEMS].sort((a, b) => b.collected_at.localeCompare(a.collected_at)).slice(offset, offset + limit).map((it) => ({
      id: it.id, title: it.title, url: it.url, language: it.language, snippet: it.cleaned_content.slice(0, 500), collected_at: it.collected_at, tags: it.tags,
    })));
  }
  if (seg[0] === "items" && seg.length === 2) {
    const i = ITEMS.findIndex((x) => x.id === seg[1]);
    if (!isUuid(seg[1]) || i < 0) return err(404, "Öğe bulunamadı");
    if (method === "DELETE") { ITEMS.splice(i, 1); return ok({ status: "deleted", items_deleted: 1, entities_deleted: 1 }); }
    const it = ITEMS[i];
    return ok({ ...it, content_hash: hashOf(it.cleaned_content).toString(16).padStart(8, "0").repeat(8), metadata: { demo: true } });
  }
  if (path === "/entities/top") {
    const t = query.get("entity_type");
    return ok(entityIndex().filter((e) => !t || e.type === t).slice(0, Number(query.get("limit") ?? 20)));
  }
  if (path === "/entities/erase") {
    const v = String(body.value).trim().toLowerCase();
    let items = 0;
    if (body.delete_items) for (let i = ITEMS.length - 1; i >= 0; i--) if (ITEMS[i].entities.some((e) => e.type === body.entity_type && e.value.toLowerCase() === v)) { ITEMS.splice(i, 1); items++; }
    const before = EDGES.length;
    for (let i = EDGES.length - 1; i >= 0; i--) if (EDGES[i].source.toLowerCase() === v || EDGES[i].target.toLowerCase() === v) EDGES.splice(i, 1);
    return ok({ items_deleted: items, entities_deleted: items ? 1 : 0, edges_deleted: before - EDGES.length });
  }
  if (path === "/admin/retention/run") return ok({ items_deleted: 0, entities_deleted: 0, default_days: 180 });

  // arama
  if (path === "/search") {
    const words = String(body.query).toLowerCase().split(/\s+/).filter(Boolean);
    return ok(ITEMS.filter((it) => words.every((w) => `${it.title} ${it.cleaned_content}`.toLowerCase().includes(w))).slice(0, Number(body.limit ?? 20)).map((it) => summary(it)));
  }
  if (path === "/search/semantic") {
    const words = String(body.query).toLowerCase().split(/\s+/).filter((w) => w.length > 2);
    return ok(ITEMS.map((it) => {
      const text = `${it.title} ${it.cleaned_content}`.toLowerCase();
      const hits = words.filter((w) => text.includes(w.slice(0, Math.max(4, w.length - 2)))).length;
      return { ...summary(it), similarity: Math.min(0.97, 0.38 + hits * 0.17 + (hashOf(it.id) % 9) / 100) };
    }).sort((a, b) => b.similarity - a.similarity).slice(0, Math.min(8, Number(body.limit ?? 20))));
  }
  if (path === "/search/entity") {
    const v = String(body.value).toLowerCase();
    return ok(ITEMS.filter((it) => it.entities.some((e) => e.type === body.entity_type && e.value.toLowerCase().includes(v))).map((it) => summary(it)));
  }

  // toplama
  if (path === "/collect") {
    const pid = String(body.plugin_id);
    const cfg = (body.config ?? {}) as J;
    if (!demoPlugins.some((p) => p.id === pid)) return err(404, "Plugin bulunamadı");
    if (pid === "username-search") {
      const u = String(cfg.username);
      const tags = (cfg.tags as string[] | undefined) ?? [];
      const pool = (demoSites as { site: string; url: string; tags: string[]; check: string }[]).filter((s) => !tags.length || s.tags.some((t) => tags.includes(t)));
      const checked = cfg.full ? Math.min(Number(cfg.max_sites ?? 200), 2515) : pool.length;
      await sleep(cfg.full ? 4500 : 2200);
      const found = pool.filter((s) => hashOf(u + s.site) % 100 < 38);
      return ok({
        items: found.length, metadata: { username: u, checked, found: found.length, unknown: Math.round(checked * 0.06) },
        results: body.include_items ? found.map((s) => ({
          url: s.url.replace("{username}", u), title: `${u} @ ${s.site}`, snippet: `Profil bulundu: ${s.site}`,
          published_at: null, metadata: { site: s.site, username: u, check: s.check }, tags: ["username", ...s.tags],
        })) : undefined,
      });
    }
    await sleep(1200);
    const url = String(cfg.url ?? cfg.feed_url ?? cfg.endpoint ?? "");
    if (/127\.0\.0\.1|localhost|^http:\/\/10\.|192\.168\./.test(url)) return err(502, "URL engellendi: iç ağ adresi (SSRF koruması)");
    const res = previewItems(pid, cfg);
    return ok({ items: res.length, metadata: { plugin: pid }, results: body.include_items ? res : undefined });
  }
  if (path === "/collect/batch") {
    await sleep(1500);
    const jobs = (body.jobs as { plugin_id: string; config: J }[]).map((j) => {
      const url = String(j.config.url ?? j.config.feed_url ?? "");
      if (/127\.0\.0\.1|localhost/.test(url)) return { plugin_id: j.plugin_id, ok: false, items: 0, error: "URL engellendi: iç ağ adresi" };
      return { plugin_id: j.plugin_id, ok: true, items: previewItems(j.plugin_id, j.config).length, error: null };
    });
    return ok({ jobs, ok: jobs.filter((j) => j.ok).length, total: jobs.length });
  }

  // kaynaklar
  if (path === "/sources" && method === "GET") {
    const q = (query.get("q") ?? "").toLowerCase(), en = query.get("enabled"), pid = query.get("plugin_id");
    return ok(SOURCES.filter((s) => (!q || `${s.name} ${s.url}`.toLowerCase().includes(q)) && (en === null || String(s.enabled) === en) && (!pid || s.plugin_id === pid))
      .sort((a, b) => String(a.name).localeCompare(String(b.name), "tr"))
      .map(({ config: _c, last: _l, created_at: _a, updated_at: _u, ...rest }) => rest));
  }
  if (path === "/sources" && method === "POST") {
    if (!demoPlugins.some((p) => p.id === body.plugin_id)) return err(404, "Plugin bulunamadı");
    const row = { id: uid(), ...body, url: body.url || null, schedule: body.schedule || null, tags: body.tags ?? [], last_crawled_at: null, last_success_at: null, failure_count: 0, avg_response_ms: null, created_at: new Date().toISOString(), updated_at: new Date().toISOString() };
    SOURCES.push(row);
    return ok({ id: row.id });
  }
  if (seg[0] === "sources" && seg.length >= 2) {
    const s = SOURCES.find((x) => x.id === seg[1]);
    if (!isUuid(seg[1]) || !s) return err(404, "Kaynak bulunamadı");
    if (seg[2] === "collect") {
      if (!s.enabled) return err(409, "Kaynak devre dışı");
      await sleep(1400);
      s.last_crawled_at = new Date().toISOString(); s.last_success_at = s.last_crawled_at; s.failure_count = 0;
      const res = previewItems(String(s.plugin_id), (s.config as J) ?? {});
      return ok({ items: res.length, metadata: {}, results: res });
    }
    if (method === "GET") {
      const { last: _l, ...rest } = s;
      return ok({ ...rest, config: s.config ?? {} });
    }
    if (method === "PATCH") {
      const { config, ...simple } = body;
      Object.assign(s, simple);
      if (config) {
        const cur = { ...((s.config as J) ?? {}) };
        for (const [k, v] of Object.entries(config as J)) { if (v === null) delete cur[k]; else if (v !== "********") cur[k] = v; }
        s.config = cur;
      }
      s.updated_at = new Date().toISOString();
      return ok({ status: "updated" });
    }
    if (method === "DELETE") { SOURCES.splice(SOURCES.indexOf(s), 1); return ok({ status: "deleted" }); }
  }

  // graf
  if (path === "/graph") {
    const g = graphState();
    return ok({ nodes: g.nodes.map((id) => ({ id })), edges: EDGES });
  }
  if (path === "/graph/relation") {
    const i = EDGES.findIndex((e) => (e.source === body.source && e.target === body.target) || (e.source === body.target && e.target === body.source));
    const edge = { source: String(body.source), target: String(body.target), relation_type: String(body.relation_type ?? "related") };
    if (i >= 0) EDGES[i] = edge; else EDGES.push(edge);
    return ok({ status: "ok" });
  }
  if (path === "/graph/analysis") {
    const { nodes, adj } = graphState();
    const n = nodes.length;
    const top = Math.min(200, Number(query.get("top") ?? 25));
    const node = query.get("node");
    return ok({
      nodes: n, edges: EDGES.length,
      centrality: nodes.map((id) => ({ id, degree: adj.get(id)?.size ?? 0, score: n > 1 ? (adj.get(id)?.size ?? 0) / (n - 1) : 1 }))
        .sort((a, b) => b.degree - a.degree || a.id.localeCompare(b.id)).slice(0, top),
      communities: communities(nodes, adj).map((c) => ({ size: c.length, members: c })),
      neighbors: node === null ? null : [...(adj.get(node) ?? [])].sort(),
    });
  }
  if (path === "/graph/export") {
    const fmt = query.get("format") ?? "graphml";
    if (fmt !== "graphml" && fmt !== "gexf") return err(400, "format graphml veya gexf olmalı");
    const { nodes } = graphState();
    const xml = fmt === "graphml"
      ? `<?xml version="1.0" encoding="utf-8"?>\n<graphml xmlns="http://graphml.graphdrawing.org/xmlns"><key id="r" for="edge" attr.name="relation_type" attr.type="string"/><graph edgedefault="undirected">\n${nodes.map((x) => `<node id="${esc(x)}"/>`).join("\n")}\n${EDGES.map((e) => `<edge source="${esc(e.source)}" target="${esc(e.target)}"><data key="r">${esc(e.relation_type)}</data></edge>`).join("\n")}\n</graph></graphml>\n`
      : `<?xml version="1.0" encoding="utf-8"?>\n<gexf xmlns="http://gexf.net/1.2" version="1.2"><graph defaultedgetype="undirected"><nodes>\n${nodes.map((x) => `<node id="${esc(x)}" label="${esc(x)}"/>`).join("\n")}\n</nodes><edges>\n${EDGES.map((e, i) => `<edge id="${i}" source="${esc(e.source)}" target="${esc(e.target)}" label="${esc(e.relation_type)}"/>`).join("\n")}\n</edges></graph></gexf>\n`;
    return new Response(xml, { headers: { "Content-Type": "application/xml" } });
  }

  // uyarılar
  if (path === "/saved-queries" && method === "GET") {
    const ae = query.get("alert_enabled");
    return ok(QUERIES.filter((q) => ae === null || String(q.alert_enabled) === ae).sort((a, b) => String(a.name).localeCompare(String(b.name), "tr")));
  }
  if (path === "/saved-queries" && method === "POST") {
    const row = { id: uid(), name: body.name, query_text: body.query_text, query_type: body.query_type, alert_enabled: body.alert_enabled, last_triggered_at: null };
    QUERIES.push(row);
    return ok({ id: row.id });
  }
  if (seg[0] === "saved-queries" && method === "DELETE") {
    const i = QUERIES.findIndex((q) => q.id === seg[1]);
    if (i < 0) return err(404, "Sorgu bulunamadı");
    QUERIES.splice(i, 1);
    return ok({ status: "deleted" });
  }
  if (path === "/alerts/test") {
    const text = `${body.title ?? ""} ${body.text}`.toLowerCase();
    const hits = QUERIES.filter((q) => q.alert_enabled && text.includes(String(q.query_text).toLowerCase()));
    hits.forEach((q) => (q.last_triggered_at = new Date().toISOString()));
    return ok(hits.map((q) => ({ query_id: q.id, query_name: q.name, item_id: null, matched: q.query_text })));
  }

  // raporlar
  if (path === "/reports/generate" || path === "/reports/markdown") {
    const fmt = path === "/reports/markdown" ? "markdown" : String(body.format ?? "markdown");
    const title = String(body.title || "OSIRIS raporu");
    const findings = (body.findings as { title: string; description: string }[]) ?? [];
    const sources = (body.sources as string[]) ?? [];
    const entities = (body.entities as { type: string; value: string }[]) ?? [];
    const gen = new Date().toISOString();
    let content = "";
    if (fmt === "markdown") {
      content = `# ${title}\n\n**Oluşturulma:** ${gen}\n**Kapsam:** ${body.scope ?? ""}\n\n## Özet\n\n${body.summary ?? ""}\n\n## Bulgular\n\n${findings.map((f, i) => `### ${i + 1}. ${f.title}\n\n${f.description}\n`).join("\n")}\n## Kaynaklar\n\n${sources.map((s) => `- ${s}`).join("\n")}\n`;
    } else if (fmt === "html") {
      content = `<!DOCTYPE html><html lang="tr"><head><meta charset="utf-8"><title>${esc(title)}</title><style>body{font-family:system-ui,sans-serif;max-width:800px;margin:2rem auto;padding:0 1rem;color:#1a1a1a}h1{border-bottom:2px solid #333}.meta{color:#666;font-size:.9rem}li{margin:.4rem 0}</style></head><body><h1>${esc(title)}</h1><p class="meta">Oluşturulma: ${gen} · Kapsam: ${esc(String(body.scope ?? ""))}</p><h2>Özet</h2><p>${esc(String(body.summary ?? ""))}</p><h2>Bulgular</h2><ul>${findings.map((f) => `<li><strong>${esc(f.title)}</strong><br>${esc(f.description)}</li>`).join("")}</ul><h2>Kaynaklar</h2><ul>${sources.map((s) => `<li>${esc(s)}</li>`).join("")}</ul></body></html>`;
    } else if (fmt === "json") {
      content = JSON.stringify({ title, generated_at: gen, scope: body.scope, summary: body.summary, findings, sources, entities }, null, 2);
    } else if (fmt === "stix") {
      const ts = gen.replace(/\.\d+Z$/, ".000Z");
      const map: Record<string, [string, string]> = { ip: ["ipv4-addr", "value"], domain: ["domain-name", "value"], email: ["email-addr", "value"], crypto_address: ["cryptocurrency-wallet", "value"] };
      const objs: J[] = [{ type: "identity", spec_version: "2.1", id: "identity--5d0a5e5c-2b1c-5e7b-9a55-0b3f8f0e6c11", created: ts, modified: ts, name: "OSIRIS", identity_class: "system" }];
      for (const e of entities) {
        if (e.type === "cve") objs.push({ type: "vulnerability", spec_version: "2.1", id: `vulnerability--${uid()}`, created: ts, modified: ts, name: e.value.toUpperCase() });
        else if (map[e.type]) objs.push({ type: "indicator", spec_version: "2.1", id: `indicator--${uid()}`, created: ts, modified: ts, pattern: `[${map[e.type][0]}:${map[e.type][1]} = '${e.value.replace(/'/g, "\\'")}']`, pattern_type: "stix", valid_from: ts, labels: ["malicious-activity"], description: `OSIRIS: ${title}` });
      }
      content = JSON.stringify({ type: "bundle", id: `bundle--${uid()}`, spec_version: "2.1", objects: objs }, null, 2);
    } else return err(400, "format markdown, html, json veya stix olmalı");
    if (path === "/reports/markdown") return ok({ markdown: content });
    const ext: Record<string, [string, string]> = { markdown: ["text/markdown", "md"], html: ["text/html", "html"], json: ["application/json", "json"], stix: ["application/stix+json", "stix.json"] };
    const stem = title.replace(/[^A-Za-z0-9_-]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "osiris-rapor";
    return ok({ format: fmt, media_type: ext[fmt][0], filename: `${stem}.${ext[fmt][1]}`, content });
  }

  return err(404, "Bu uç demo modunda yok");
}

export function installDemo() {
  const real = window.fetch.bind(window);
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const raw = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const u = new URL(raw, location.href);
    const i = u.pathname.indexOf("/api/");
    if (i < 0) return real(input, init);
    const path = u.pathname.slice(i + 4);
    const method = (init?.method ?? "GET").toUpperCase();
    let body: J = {};
    try { body = init?.body ? JSON.parse(String(init.body)) : {}; } catch { /* boş */ }
    await sleep(120 + Math.random() * 180);
    if (init?.signal?.aborted) throw new DOMException("Aborted", "AbortError");
    return route(method, path, u.searchParams, body);
  };
  // Demo girişini kolaylaştır: oturum yoksa yönetici olarak aç (çıkışta herhangi bir anahtar çalışır)
  if (!session.role) session.save({ key: "demo" }, "admin");
}
