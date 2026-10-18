// Plugin manifest'lerindeki config_schema'dan form üretir (GET /plugins).
// Alan tipleri: string, integer, number, boolean, array (virgülle), object (JSON).
import type { SchemaField } from "./api";
import { Label } from "./ui";

export type FormValues = Record<string, string | boolean>;

const SECRET_KEYS = /(api_key|access_token|token|password|secret)/i;

export function initialValues(schema: Record<string, SchemaField>): FormValues {
  const v: FormValues = {};
  for (const [k, f] of Object.entries(schema)) {
    if (f.type === "boolean") v[k] = f.default === true;
    else v[k] = "";
  }
  return v;
}

function placeholderFor(f: SchemaField): string {
  if (f.default === undefined || f.default === null) {
    if (f.type === "array") return "virgülle ayırın";
    if (f.type === "object") return '{"anahtar": "değer"}';
    return "";
  }
  if (Array.isArray(f.default)) return `varsayılan: ${f.default.join(", ")}`;
  if (typeof f.default === "object") return `varsayılan: ${JSON.stringify(f.default)}`;
  return `varsayılan: ${String(f.default)}`;
}

/** Form değerlerini API config'ine çevirir. Boş alanlar gönderilmez (sunucu varsayılanı geçerli). */
export function toConfig(schema: Record<string, SchemaField>, values: FormValues):
  { config: Record<string, unknown>; errors: Record<string, string> } {
  const config: Record<string, unknown> = {};
  const errors: Record<string, string> = {};
  for (const [k, f] of Object.entries(schema)) {
    const raw = values[k];
    if (f.type === "boolean") {
      if (raw !== (f.default === true)) config[k] = !!raw;
      continue;
    }
    const s = typeof raw === "string" ? raw.trim() : "";
    if (!s) {
      if (f.required) errors[k] = "Zorunlu alan";
      continue;
    }
    switch (f.type) {
      case "integer": {
        if (!/^-?\d+$/.test(s)) errors[k] = "Tam sayı girin";
        else config[k] = parseInt(s, 10);
        break;
      }
      case "number": {
        const n = Number(s.replace(",", "."));
        if (Number.isNaN(n)) errors[k] = "Sayı girin";
        else config[k] = n;
        break;
      }
      case "array":
        config[k] = s.split(/[,\n]/).map((x) => x.trim()).filter(Boolean);
        break;
      case "object":
        try {
          const o = JSON.parse(s);
          if (o === null || typeof o !== "object" || Array.isArray(o)) errors[k] = "JSON nesnesi girin";
          else config[k] = o;
        } catch {
          errors[k] = "Geçerli JSON değil";
        }
        break;
      default:
        config[k] = s;
    }
  }
  return { config, errors };
}

/** Kaynak tanımındaki URL kutusu bu alanları besleyebilir (API de aynı eşlemeyi yapar). */
export const URL_KEYS = ["url", "feed_url", "endpoint"];

export function SchemaForm({ schema, values, onChange, errors, idPrefix, omit = [] }: {
  schema: Record<string, SchemaField>;
  values: FormValues;
  onChange: (v: FormValues) => void;
  errors: Record<string, string>;
  idPrefix: string;
  omit?: string[];
}) {
  const entries = Object.entries(schema).filter(([k]) => !omit.includes(k));
  if (entries.length === 0) return <p className="text-sm text-osiris-muted">Bu plugin ayar istemiyor.</p>;
  // Zorunlu alanlar önce
  entries.sort(([, a], [, b]) => Number(!!b.required) - Number(!!a.required));
  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
      {entries.map(([key, f]) => {
        const id = `${idPrefix}-${key}`;
        const err = errors[key];
        const set = (val: string | boolean) => onChange({ ...values, [key]: val });
        const hint = err ? <span className="text-osiris-bad">{err}</span> : f.description;
        if (f.type === "boolean") {
          return (
            <div key={key} className="flex flex-col justify-end">
              <label htmlFor={id} className="inline-flex cursor-pointer items-center gap-2 text-sm">
                <input id={id} type="checkbox" checked={!!values[key]} onChange={(e) => set(e.target.checked)} className="h-4 w-4 accent-cyan-400" />
                <span className="font-mono text-[13px]">{key}</span>
              </label>
              {f.description && <p className="mt-1 text-xs text-osiris-faint">{f.description}</p>}
            </div>
          );
        }
        const common = {
          id,
          value: String(values[key] ?? ""),
          placeholder: placeholderFor(f),
          "aria-invalid": !!err,
          className: `field font-mono text-[13px] ${err ? "border-osiris-bad" : ""}`,
        };
        return (
          <div key={key} className={f.type === "object" ? "sm:col-span-2" : ""}>
            <Label label={key} required={f.required} hint={hint} htmlFor={id}>
              {f.type === "object" ? (
                <textarea {...common} rows={3} onChange={(e) => set(e.target.value)} />
              ) : (
                <input
                  {...common}
                  type={SECRET_KEYS.test(key) ? "password" : "text"}
                  inputMode={f.type === "integer" ? "numeric" : f.type === "number" ? "decimal" : undefined}
                  autoComplete="off"
                  onChange={(e) => set(e.target.value)}
                />
              )}
            </Label>
          </div>
        );
      })}
    </div>
  );
}

/** Kayıtlı config'ten form değerleri (kaynak düzenleme). Sır alanları maskeli gelir ve
 *  maske olduğu gibi geri gönderilirse sunucu eski değeri korur. */
export function valuesFromConfig(schema: Record<string, SchemaField>, config: Record<string, unknown>): FormValues {
  const v = initialValues(schema);
  for (const [k, f] of Object.entries(schema)) {
    const c = config[k];
    if (c === undefined || c === null) continue;
    if (f.type === "boolean") v[k] = c === true;
    else if (Array.isArray(c)) v[k] = c.join(", ");
    else if (typeof c === "object") v[k] = JSON.stringify(c);
    else v[k] = String(c);
  }
  return v;
}

/** Düzenlemede boolean alanlar varsayılana dönse bile açıkça gönderilir. */
export function toConfigForUpdate(schema: Record<string, SchemaField>, values: FormValues) {
  const out = toConfig(schema, values);
  for (const [k, f] of Object.entries(schema)) if (f.type === "boolean") out.config[k] = !!values[k];
  return out;
}
