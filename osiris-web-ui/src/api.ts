// OSIRIS API istemcisi — aynı köken /api (nginx) veya Vite dev proxy'si.
// Uç başvurusu: docs/API.tr.md. Tüm tipler sunucunun gerçek yanıtlarından alınmıştır.

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

const KEY = "osiris_key";
const JWT = "osiris_jwt";
const ROLE = "osiris_role";

// Oturum sekmeye özeldir (sessionStorage). Depolama kapalıysa (gizli pencere, gömülü
// çerçeve) bellekte tutulur: sekme yenilenince yeniden giriş gerekir ama uygulama çökmez.
const memory = new Map<string, string>();
const store = {
  get(k: string): string | null {
    try {
      return sessionStorage.getItem(k);
    } catch {
      return memory.get(k) ?? null;
    }
  },
  set(k: string, v: string) {
    try {
      sessionStorage.setItem(k, v);
    } catch {
      memory.set(k, v);
    }
  },
  del(k: string) {
    try {
      sessionStorage.removeItem(k);
    } catch {
      /* yok say */
    }
    memory.delete(k);
  },
};

export const session = {
  get key() {
    return store.get(KEY) ?? "";
  },
  get jwt() {
    return store.get(JWT) ?? "";
  },
  get role() {
    return store.get(ROLE) as Role | null;
  },
  save(cred: { key?: string; jwt?: string }, role: Role) {
    store.del(KEY);
    store.del(JWT);
    if (cred.key) store.set(KEY, cred.key);
    if (cred.jwt) store.set(JWT, cred.jwt);
    store.set(ROLE, role);
  },
  clear() {
    store.del(KEY);
    store.del(JWT);
    store.del(ROLE);
  },
};

/** 401 alındığında kabuk girişe döner. */
let onUnauthorized: () => void = () => undefined;
export function setUnauthorizedHandler(fn: () => void) {
  onUnauthorized = fn;
}

export function credentialHeaders(cred?: { key?: string; jwt?: string }): Record<string, string> {
  // API anahtarı önceliklidir (rol sunucuda çözülür); JWT yedek.
  const key = cred ? cred.key : session.key;
  const jwt = cred ? cred.jwt : session.jwt;
  if (key) return { "X-API-Key": key };
  if (jwt) return { Authorization: `Bearer ${jwt}` };
  return {};
}

// Kullanıcı adı taraması 2500 sitede dakikalar sürebilir; diğer istekler kısa.
const DEFAULT_TIMEOUT_MS = 30_000;
export const LONG_TIMEOUT_MS = 15 * 60_000;

interface Opts extends RequestInit {
  timeoutMs?: number;
  cred?: { key?: string; jwt?: string };
  /** true → gövde metin olarak döner (XML dışa aktarımları). */
  raw?: boolean;
}

export async function api<T>(path: string, opts: Opts = {}): Promise<T> {
  const { timeoutMs = DEFAULT_TIMEOUT_MS, cred, raw, ...init } = opts;
  const ctrl = new AbortController();
  const outer = init.signal;
  if (outer) outer.addEventListener("abort", () => ctrl.abort(), { once: true });
  const timer = setTimeout(() => ctrl.abort("timeout"), timeoutMs);
  let res: Response;
  try {
    res = await fetch(`/api${path}`, {
      ...init,
      signal: ctrl.signal,
      headers: {
        "Content-Type": "application/json",
        ...credentialHeaders(cred),
        ...(init.headers ?? {}),
      },
    });
  } catch (err) {
    clearTimeout(timer);
    if (outer?.aborted) throw new ApiError(0, "İstek iptal edildi");
    if (err instanceof DOMException && err.name === "AbortError") {
      throw new ApiError(0, `İstek zaman aşımına uğradı (${Math.round(timeoutMs / 1000)} sn)`);
    }
    throw new ApiError(0, "Sunucuya ulaşılamıyor. Ağ bağlantısını ve API servisini kontrol edin.");
  }
  clearTimeout(timer);
  if (!res.ok) {
    let detail = `HTTP ${res.status}`;
    try {
      const body = await res.json();
      if (typeof body?.detail === "string") detail = body.detail;
      else if (Array.isArray(body?.detail)) {
        detail = body.detail
          .map((d: { loc?: unknown[]; msg?: string }) => `${(d.loc ?? []).slice(1).join(".")}: ${d.msg}`)
          .join("; ");
      }
    } catch {
      /* düz metin gövde */
    }
    if (res.status === 401 && !cred) {
      session.clear();
      onUnauthorized();
      detail = "Oturum geçersiz. Yeniden giriş yapın.";
    }
    throw new ApiError(res.status, detail);
  }
  if (res.status === 204) return undefined as T;
  if (raw) return (await res.text()) as T;
  return (await res.json()) as T;
}

export const apiText = (path: string) => api<string>(path, { raw: true, timeoutMs: 120_000 });

const get = <T,>(path: string, o?: Opts) => api<T>(path, o);
const post = <T,>(path: string, body: unknown, o?: Opts) =>
  api<T>(path, { ...o, method: "POST", body: JSON.stringify(body) });
const del = <T,>(path: string) => api<T>(path, { method: "DELETE" });

function qs(params: Record<string, string | number | boolean | undefined | null>) {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === "") continue;
    p.set(k, String(v));
  }
  const s = p.toString();
  return s ? `?${s}` : "";
}

// --------------------------------------------------------------------- tipler

export type Role = "viewer" | "analyst" | "admin";
export const ROLE_RANK: Record<Role, number> = { viewer: 1, analyst: 2, admin: 3 };
export const can = (role: Role | null, minimum: Role) =>
  !!role && ROLE_RANK[role] >= ROLE_RANK[minimum];

export const ENTITY_TYPES = [
  "person", "org", "location", "ip", "domain", "email", "phone",
  "crypto_address", "hash", "username", "cve", "custom",
] as const;
export type EntityType = (typeof ENTITY_TYPES)[number];

export const ENTITY_LABEL: Record<EntityType, string> = {
  person: "Kişi", org: "Kurum", location: "Yer", ip: "IP", domain: "Alan adı",
  email: "E-posta", phone: "Telefon", crypto_address: "Kripto adresi", hash: "Hash",
  username: "Kullanıcı adı", cve: "CVE", custom: "Diğer",
};

export const NETWORK_TYPES = [
  "www", "tor", "i2p", "p2p", "freenet", "zeronet", "irc", "matrix", "rss", "api",
  "blockchain", "sdr", "custom",
] as const;

export const QUERY_TYPES = ["fts", "semantic", "regex", "entity", "graph"] as const;
export const QUERY_LABEL: Record<string, string> = {
  fts: "Tam metin", semantic: "Semantik", regex: "Regex", entity: "Varlık", graph: "Graf",
};

export interface Stats {
  sources: number;
  sources_enabled: number;
  items: number;
  entities: number;
  edges: number;
  saved_queries: number;
  alerts: number;
  latest_item_at: string | null;
}

export interface SchemaField {
  type?: string;
  required?: boolean;
  default?: unknown;
  description?: string;
}

export interface Plugin {
  id: string;
  name: string;
  network_type: string;
  description: string;
  config_schema: Record<string, SchemaField>;
  schedule_default: string;
}

export interface Source {
  id: string;
  name: string;
  url: string | null;
  network_type: string;
  plugin_id: string;
  schedule: string | null;
  priority: number;
  enabled: boolean;
  tags: string[] | null;
  last_crawled_at: string | null;
  last_success_at: string | null;
  failure_count: number;
  avg_response_ms: number | null;
  retention_days: number | null;
}

export interface SourceInput {
  name: string;
  plugin_id: string;
  url?: string;
  network_type: string;
  schedule?: string;
  priority?: number;
  enabled?: boolean;
  tags?: string[];
  config: Record<string, unknown>;
  retention_days?: number | null;
}

export interface ItemSummary {
  id: string;
  title: string | null;
  url: string | null;
  language?: string | null;
  snippet?: string | null;
  cleaned_content?: string | null;
  collected_at?: string | null;
  tags?: string[] | null;
}

export interface ItemDetail {
  id: string;
  source_id: string | null;
  title: string | null;
  url: string | null;
  language: string | null;
  cleaned_content: string | null;
  collected_at: string | null;
  published_at: string | null;
  content_hash: string;
  metadata: Record<string, unknown> | null;
  tags: string[] | null;
  entities: { type: EntityType; value: string; mention_count: number }[];
}

export interface TopEntity {
  type: EntityType;
  value: string;
  mentions: number;
  last_seen: string | null;
}

export interface CollectedPreview {
  url: string | null;
  title: string | null;
  snippet: string;
  published_at: string | null;
  metadata: Record<string, unknown>;
  tags: string[];
}

export interface CollectResult {
  items: number;
  metadata: Record<string, unknown>;
  results?: CollectedPreview[];
}

export interface BatchResult {
  jobs: { plugin_id: string; ok: boolean; items?: number; error: string | null }[];
  ok: number;
  total: number;
}

export interface GraphData {
  nodes: { id: string; [k: string]: unknown }[];
  edges: { source: string; target: string; relation_type?: string; weight?: number }[];
}

export interface SavedQuery {
  id: string;
  name: string;
  query_text: string;
  query_type: string;
  alert_enabled: boolean;
  last_triggered_at: string | null;
}

export interface AlertMatch {
  query_id: string | null;
  query_name: string;
  item_id: string | null;
  matched: string;
}

export interface ApiKeyRow {
  id: string;
  username: string;
  role: Role;
  key_prefix: string;
  name: string;
  revoked: boolean;
  created_at: string;
  last_used_at: string | null;
}

export interface Finding {
  title: string;
  description: string;
  severity?: string;
}

export interface SourceDetail extends Source {
  config: Record<string, unknown>;
  created_at: string | null;
  updated_at: string | null;
}

export type SourceUpdate = Partial<{
  name: string;
  url: string;
  schedule: string;
  priority: number;
  enabled: boolean;
  tags: string[];
  config: Record<string, unknown>;
  retention_days: number | null;
}>;

/** GET /sources/{id} sır alanlarını bu maskeyle döndürür; PATCH'te maske eski değeri korur. */
export const SECRET_MASK = "********";

export interface GraphAnalysis {
  nodes: number;
  edges: number;
  centrality: { id: string; score: number; degree: number }[];
  communities: { size: number; members: string[] }[] | null;
  neighbors: string[] | null;
}

export type ReportFormat = "markdown" | "html" | "json" | "stix";

export interface ReportOut {
  format: ReportFormat;
  media_type: string;
  filename: string;
  content: string;
}

// ----------------------------------------------------------------------- uçlar

export const Api = {
  health: () => get<{ status: string; service: string }>("/health"),
  stats: () => get<Stats>("/stats"),
  plugins: () => get<Plugin[]>("/plugins"),

  collect: (plugin_id: string, config: Record<string, unknown>, opts?: { signal?: AbortSignal; long?: boolean }) =>
    post<CollectResult>(
      "/collect",
      { plugin_id, config, include_items: true },
      { signal: opts?.signal, timeoutMs: opts?.long ? LONG_TIMEOUT_MS : 5 * 60_000 },
    ),
  collectBatch: (jobs: { plugin_id: string; config: Record<string, unknown> }[]) =>
    post<BatchResult>("/collect/batch", { jobs }, { timeoutMs: LONG_TIMEOUT_MS }),

  search: (query: string, limit = 50) => post<ItemSummary[]>("/search", { query, limit }),
  searchSemantic: (query: string, limit = 50) =>
    post<(ItemSummary & { similarity: number })[]>("/search/semantic", { query, limit }, { timeoutMs: 60_000 }),
  searchEntity: (entity_type: string, value: string, limit = 50) =>
    post<ItemSummary[]>("/search/entity", { entity_type, value, limit }),

  recentItems: (limit = 25, offset = 0) => get<ItemSummary[]>(`/items/recent${qs({ limit, offset })}`),
  item: (id: string) => get<ItemDetail>(`/items/${encodeURIComponent(id)}`),
  deleteItem: (id: string) =>
    del<{ status: string; items_deleted: number; entities_deleted: number }>(`/items/${encodeURIComponent(id)}`),
  topEntities: (entity_type?: string, limit = 50) =>
    get<TopEntity[]>(`/entities/top${qs({ entity_type, limit })}`),
  eraseEntity: (entity_type: string, value: string, delete_items: boolean) =>
    post<{ items_deleted: number; entities_deleted: number; edges_deleted: number }>(
      "/entities/erase", { entity_type, value, delete_items }),
  runRetention: () =>
    post<{ items_deleted: number; entities_deleted: number; default_days?: number }>("/admin/retention/run", {}),

  sources: (f: { enabled?: boolean; plugin_id?: string; q?: string; limit?: number; offset?: number } = {}) =>
    get<Source[]>(`/sources${qs({ limit: 200, ...f })}`),
  createSource: (s: SourceInput) => post<{ id: string }>("/sources", s),
  source: (id: string) => get<SourceDetail>(`/sources/${encodeURIComponent(id)}`),
  updateSource: (id: string, patch: SourceUpdate) =>
    api<{ status: string }>(`/sources/${encodeURIComponent(id)}`, { method: "PATCH", body: JSON.stringify(patch) }),
  deleteSource: (id: string) => del<{ status: string }>(`/sources/${encodeURIComponent(id)}`),
  runSource: (id: string) =>
    post<CollectResult>(`/sources/${encodeURIComponent(id)}/collect?include_items=true`, {},
      { timeoutMs: LONG_TIMEOUT_MS }),

  graph: () => get<GraphData>("/graph"),
  graphAnalysis: (top = 25, node?: string) => get<GraphAnalysis>(`/graph/analysis${qs({ top, node })}`),
  graphExport: (format: "graphml" | "gexf") => apiText(`/graph/export${qs({ format })}`),
  addRelation: (source: string, target: string, relation_type: string) =>
    post<{ status: string }>("/graph/relation", { source, target, relation_type }),

  savedQueries: (alert_enabled?: boolean) =>
    get<SavedQuery[]>(`/saved-queries${qs({ alert_enabled, limit: 200 })}`),
  createSavedQuery: (q: { name: string; query_text: string; query_type: string; alert_enabled: boolean }) =>
    post<{ id: string }>("/saved-queries", q),
  deleteSavedQuery: (id: string) => del<{ status: string }>(`/saved-queries/${encodeURIComponent(id)}`),
  testAlerts: (text: string, title = "") => post<AlertMatch[]>("/alerts/test", { text, title }),

  reportMarkdown: (r: { title: string; scope: string; summary: string; findings: Finding[]; sources: string[] }) =>
    post<{ markdown: string }>("/reports/markdown", r),
  reportGenerate: (r: {
    title: string; scope: string; summary: string; findings: Finding[]; sources: string[];
    format: ReportFormat; entities?: { type: string; value: string }[]; identity_name?: string;
  }) => post<ReportOut>("/reports/generate", r),

  keys: () => get<ApiKeyRow[]>("/auth/keys"),
  createKey: (username: string, role: Role, name: string) =>
    post<{ id: string; api_key: string; role: Role }>("/auth/keys", { username, role, name }),
  revokeKey: (id: string) => del<{ status: string }>(`/auth/keys/${encodeURIComponent(id)}`),
  token: (api_key: string, role: Role) =>
    post<{ access_token: string; token_type: string; expires_in: number }>("/auth/token", { api_key, role }),
};

/** Sunucunun rolü bildiren bir ucu yok; yan etkisiz yoklamalarla çözülür.
 *  admin: GET /auth/keys → 200. analyst: POST /collect (var olmayan plugin) → 404,
 *  viewer aynı çağrıda 403 alır (rol denetimi plugin denetiminden önce çalışır). */
export async function resolveRole(cred: { key?: string; jwt?: string }): Promise<Role> {
  const me = await api<{ user: string; role: string }>("/auth/me", { cred, timeoutMs: 15_000 });
  if (me.role in ROLE_RANK) return me.role as Role;
  throw new Error(`Bilinmeyen rol: ${me.role}`);
}
