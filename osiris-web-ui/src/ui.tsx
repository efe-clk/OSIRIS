import {
  createContext, useCallback, useContext, useEffect, useRef, useState,
  type ReactNode,
} from "react";
import type { EntityType } from "./api";
import { ENTITY_LABEL } from "./api";
import { errMsg } from "./util";

// --------------------------------------------------------------- sayfa iskeleti

export function PageHeader({ title, lede, actions }: { title: string; lede?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div className="min-w-0">
        <h1 className="text-[1.6rem] font-semibold leading-tight tracking-tight">{title}</h1>
        {lede && <p className="mt-1 max-w-[70ch] text-sm text-osiris-muted">{lede}</p>}
      </div>
      {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
    </header>
  );
}

export function Section({ title, aside, children, className = "" }: {
  title: string; aside?: ReactNode; children: ReactNode; className?: string;
}) {
  return (
    <section className={`min-w-0 ${className}`}>
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <h2 className="text-base font-semibold">{title}</h2>
        {aside && <div className="text-sm text-osiris-muted">{aside}</div>}
      </div>
      {children}
    </section>
  );
}

export function Notice({ kind = "error", children, onClose }: {
  kind?: "error" | "info" | "ok" | "warn"; children: ReactNode; onClose?: () => void;
}) {
  const tone = {
    error: "border-osiris-bad/40 bg-osiris-bad/10 text-red-200",
    info: "border-osiris-accent/30 bg-osiris-accent/10 text-cyan-100",
    ok: "border-osiris-ok/30 bg-osiris-ok/10 text-green-100",
    warn: "border-osiris-warn/40 bg-osiris-warn/10 text-amber-100",
  }[kind];
  return (
    <div role={kind === "error" ? "alert" : "status"} className={`mb-4 flex items-start gap-3 rounded-md border px-3.5 py-2.5 text-sm ${tone}`}>
      <div className="min-w-0 flex-1 break-words">{children}</div>
      {onClose && (
        <button onClick={onClose} className="shrink-0 text-xs opacity-70 hover:opacity-100" aria-label="Kapat">
          Kapat
        </button>
      )}
    </div>
  );
}

export function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="rounded-lg border border-dashed border-osiris-line px-6 py-10 text-center">
      <p className="font-medium">{title}</p>
      {children && <div className="mx-auto mt-1 max-w-[52ch] text-sm text-osiris-muted">{children}</div>}
    </div>
  );
}

export function Spinner({ label }: { label?: string }) {
  return (
    <span className="inline-flex items-center gap-2 text-sm text-osiris-muted">
      <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-osiris-line border-t-osiris-accent" />
      {label}
    </span>
  );
}

export function Loading() {
  return (
    <div className="py-10">
      <Spinner label="Yükleniyor" />
    </div>
  );
}

// ------------------------------------------------------------------ rozetler

const ENTITY_TONE: Record<string, string> = {
  person: "text-violet-300 border-violet-400/30",
  org: "text-sky-300 border-sky-400/30",
  location: "text-emerald-300 border-emerald-400/30",
  ip: "text-orange-300 border-orange-400/30",
  domain: "text-cyan-300 border-cyan-400/30",
  email: "text-pink-300 border-pink-400/30",
  phone: "text-lime-300 border-lime-400/30",
  crypto_address: "text-amber-300 border-amber-400/30",
  hash: "text-slate-300 border-slate-400/30",
  username: "text-teal-300 border-teal-400/30",
  cve: "text-red-300 border-red-400/30",
  custom: "text-slate-300 border-slate-400/30",
};

export function EntityTypeTag({ type }: { type: EntityType | string }) {
  return (
    <span className={`inline-block whitespace-nowrap rounded border px-1.5 py-px text-[11px] ${ENTITY_TONE[type] ?? ENTITY_TONE.custom}`}>
      {ENTITY_LABEL[type as EntityType] ?? type}
    </span>
  );
}

export function Pill({ tone = "idle", children }: { tone?: "ok" | "bad" | "warn" | "idle" | "accent"; children: ReactNode }) {
  const cls = {
    ok: "bg-osiris-ok/10 text-osiris-ok",
    bad: "bg-osiris-bad/10 text-osiris-bad",
    warn: "bg-osiris-warn/10 text-osiris-warn",
    idle: "border border-osiris-line text-osiris-muted",
    accent: "bg-osiris-accent/10 text-osiris-accent",
  }[tone];
  return <span className={`inline-flex items-center gap-1 whitespace-nowrap rounded-full px-2 py-0.5 text-xs ${cls}`}>{children}</span>;
}

// ------------------------------------------------------------------- formlar

export function Label({ label, hint, required, children, htmlFor }: {
  label: string; hint?: ReactNode; required?: boolean; children: ReactNode; htmlFor?: string;
}) {
  return (
    <div className="min-w-0">
      <label htmlFor={htmlFor} className="mb-1 block text-sm text-osiris-muted">
        {label}
        {required && <span className="text-osiris-accent"> *</span>}
      </label>
      {children}
      {hint && <p className="mt-1 text-xs text-osiris-faint">{hint}</p>}
    </div>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: {
  value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; label: string;
}) {
  return (
    <div role="radiogroup" aria-label={label} className="inline-flex overflow-hidden rounded-md border border-osiris-line">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={`border-r border-osiris-line px-3 py-1.5 text-sm last:border-r-0 ${
            value === o.value ? "bg-osiris-accent/15 text-osiris-accent" : "text-osiris-muted hover:text-osiris-ink"
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Toggle({ checked, onChange, label, id }: { checked: boolean; onChange: (v: boolean) => void; label: string; id: string }) {
  return (
    <label htmlFor={id} className="inline-flex cursor-pointer items-center gap-2 text-sm">
      <input id={id} type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)}
        className="h-4 w-4 accent-cyan-400" />
      {label}
    </label>
  );
}

// ------------------------------------------------------------ katmanlar

export function Drawer({ open, onClose, title, children, wide }: {
  open: boolean; onClose: () => void; title: ReactNode; children: ReactNode; wide?: boolean;
}) {
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-40 flex justify-end" role="dialog" aria-modal="true">
      <button className="absolute inset-0 bg-black/60" onClick={onClose} aria-label="Kapat" />
      <div className={`relative flex h-full w-full flex-col border-l border-osiris-line bg-osiris-bg shadow-2xl ${wide ? "max-w-3xl" : "max-w-xl"}`}>
        <div className="flex items-start justify-between gap-4 border-b border-osiris-line px-5 py-4">
          <div className="min-w-0 text-lg font-semibold">{title}</div>
          <button onClick={onClose} className="btn-ghost px-2 py-1 text-xs">Kapat</button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
      </div>
    </div>
  );
}

interface ConfirmReq {
  title: string;
  body?: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  resolve: (ok: boolean) => void;
}

interface ToastItem { id: number; text: string; tone: "ok" | "error" }

const UiContext = createContext<{
  confirm: (r: Omit<ConfirmReq, "resolve">) => Promise<boolean>;
  toast: (text: string, tone?: "ok" | "error") => void;
}>({ confirm: async () => false, toast: () => undefined });

export function UiProvider({ children }: { children: ReactNode }) {
  const [req, setReq] = useState<ConfirmReq | null>(null);
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const seq = useRef(0);
  const confirmBtn = useRef<HTMLButtonElement>(null);

  const confirm = useCallback(
    (r: Omit<ConfirmReq, "resolve">) => new Promise<boolean>((resolve) => setReq({ ...r, resolve })),
    [],
  );
  const toast = useCallback((text: string, tone: "ok" | "error" = "ok") => {
    const id = ++seq.current;
    setToasts((t) => [...t, { id, text, tone }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4500);
  }, []);

  useEffect(() => {
    if (req) confirmBtn.current?.focus();
  }, [req]);

  const close = (ok: boolean) => {
    req?.resolve(ok);
    setReq(null);
  };

  return (
    <UiContext.Provider value={{ confirm, toast }}>
      {children}
      {req && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4" role="alertdialog" aria-modal="true"
          onKeyDown={(e) => e.key === "Escape" && close(false)}>
          <button className="absolute inset-0 bg-black/70" onClick={() => close(false)} aria-label="Vazgeç" />
          <div className="panel relative w-full max-w-md p-5">
            <h2 className="text-lg font-semibold">{req.title}</h2>
            {req.body && <div className="mt-2 text-sm text-osiris-muted">{req.body}</div>}
            <div className="mt-5 flex justify-end gap-2">
              <button className="btn-ghost" onClick={() => close(false)}>Vazgeç</button>
              <button ref={confirmBtn} className={req.danger ? "btn btn-danger bg-osiris-bad/15" : "btn-primary"} onClick={() => close(true)}>
                {req.confirmLabel}
              </button>
            </div>
          </div>
        </div>
      )}
      <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex max-w-sm flex-col gap-2" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`pointer-events-auto rounded-md border px-4 py-2.5 text-sm shadow-lg ${
            t.tone === "ok" ? "border-osiris-ok/30 bg-[#10201a] text-green-100" : "border-osiris-bad/40 bg-[#2a1416] text-red-100"
          }`}>
            {t.text}
          </div>
        ))}
      </div>
    </UiContext.Provider>
  );
}

export const useUi = () => useContext(UiContext);

// ---------------------------------------------------------------- veri kancası

export function useLoad<T>(fn: () => Promise<T>, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let alive = true;
    setLoading(true);
    fn()
      .then((d) => {
        if (!alive) return;
        setData(d);
        setError(null);
      })
      .catch((e) => alive && setError(errMsg(e)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick]);
  const reload = useCallback(() => setTick((t) => t + 1), []);
  return { data, error, loading, reload, setData };
}

export function Pager({ offset, limit, count, onChange }: {
  offset: number; limit: number; count: number; onChange: (offset: number) => void;
}) {
  return (
    <div className="mt-4 flex items-center justify-between gap-3 text-sm text-osiris-muted">
      <span className="tnum">
        {count === 0 ? "Kayıt yok" : `${offset + 1}–${offset + count}`}
      </span>
      <div className="flex gap-2">
        <button className="btn-ghost py-1" disabled={offset === 0} onClick={() => onChange(Math.max(0, offset - limit))}>
          Önceki
        </button>
        <button className="btn-ghost py-1" disabled={count < limit} onClick={() => onChange(offset + limit)}>
          Sonraki
        </button>
      </div>
    </div>
  );
}

export function KeyValue({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[minmax(0,9rem)_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-osiris-muted">{k}</dt>
          <dd className="min-w-0 break-words">{v}</dd>
        </div>
      ))}
    </dl>
  );
}
