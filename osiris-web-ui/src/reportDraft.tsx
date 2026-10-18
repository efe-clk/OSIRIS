// Soruşturma sırasında ekranlardan "Rapora ekle" ile biriken bulgular.
// Taslak yalnızca bu tarayıcıda tutulur (localStorage); rapor sayfası bunları kullanır.
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import type { Finding } from "./api";

export interface Draft {
  title: string;
  scope: string;
  summary: string;
  findings: Finding[];
  sources: string[];
  /** STIX/JSON için göstergeler (Varlıklar sayfasından eklenir). */
  entities: { type: string; value: string }[];
}

const STORE = "osiris_report_draft";
const EMPTY: Draft = { title: "", scope: "", summary: "", findings: [], sources: [], entities: [] };

function load(): Draft {
  try {
    const raw = localStorage.getItem(STORE);
    if (!raw) return EMPTY;
    const d = JSON.parse(raw);
    return { ...EMPTY, ...d, findings: Array.isArray(d.findings) ? d.findings : [], sources: Array.isArray(d.sources) ? d.sources : [], entities: Array.isArray(d.entities) ? d.entities : [] };
  } catch {
    return EMPTY;
  }
}

const Ctx = createContext<{
  draft: Draft;
  setDraft: (d: Draft) => void;
  addFinding: (f: Finding, source?: string | null) => void;
  addEntities: (es: { type: string; value: string }[]) => void;
  clear: () => void;
}>({ draft: EMPTY, setDraft: () => undefined, addFinding: () => undefined, addEntities: () => undefined, clear: () => undefined });

export function ReportDraftProvider({ children }: { children: ReactNode }) {
  const [draft, setDraft] = useState<Draft>(load);
  useEffect(() => {
    try {
      localStorage.setItem(STORE, JSON.stringify(draft));
    } catch {
      /* depolama kapalı: taslak oturum boyunca bellekte kalır */
    }
  }, [draft]);
  const addFinding = useCallback((f: Finding, source?: string | null) => {
    setDraft((d) => ({
      ...d,
      findings: [...d.findings, f].slice(0, 500),
      sources: source && !d.sources.includes(source) ? [...d.sources, source].slice(0, 500) : d.sources,
    }));
  }, []);
  const addEntities = useCallback((es: { type: string; value: string }[]) => {
    setDraft((d) => {
      const seen = new Set(d.entities.map((e) => `${e.type}:${e.value.toLowerCase()}`));
      const add = es.filter((e) => !seen.has(`${e.type}:${e.value.toLowerCase()}`));
      return { ...d, entities: [...d.entities, ...add].slice(0, 500) };
    });
  }, []);
  const clear = useCallback(() => setDraft(EMPTY), []);
  return <Ctx.Provider value={{ draft, setDraft, addFinding, addEntities, clear }}>{children}</Ctx.Provider>;
}

export const useReportDraft = () => useContext(Ctx);
