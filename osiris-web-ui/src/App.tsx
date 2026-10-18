import { useEffect, useState } from "react";
import { HashRouter, NavLink, Route, Routes, useLocation } from "react-router-dom";
import { Api, can, type Role } from "./api";
import { AuthProvider, useAuth } from "./auth";
import { ReportDraftProvider, useReportDraft } from "./reportDraft";
import { UiProvider } from "./ui";
import Admin from "./pages/Admin";
import Alerts from "./pages/Alerts";
import Collect from "./pages/Collect";
import Dashboard from "./pages/Dashboard";
import Entities from "./pages/Entities";
import Graph from "./pages/Graph";
import Items from "./pages/Items";
import Login from "./pages/Login";
import Reports from "./pages/Reports";
import Search from "./pages/Search";
import Sources from "./pages/Sources";
import UsernameScan from "./pages/UsernameScan";

interface NavItem { to: string; label: string; min?: Role }

const NAV: { group: string; items: NavItem[] }[] = [
  {
    group: "Soruşturma",
    items: [
      { to: "/", label: "Panel" },
      { to: "/username", label: "Kullanıcı adı tarama", min: "analyst" },
      { to: "/collect", label: "Toplama", min: "analyst" },
      { to: "/search", label: "Arama" },
    ],
  },
  {
    group: "Veri",
    items: [
      { to: "/items", label: "Öğeler" },
      { to: "/entities", label: "Varlıklar" },
      { to: "/graph", label: "İlişki grafı" },
      { to: "/sources", label: "Kaynaklar" },
    ],
  },
  {
    group: "Çıktı",
    items: [
      { to: "/alerts", label: "Uyarı kuralları" },
      { to: "/reports", label: "Raporlar" },
    ],
  },
  { group: "Yönetim", items: [{ to: "/admin", label: "Erişim ve gizlilik", min: "admin" }] },
];

const ROLE_LABEL: Record<Role, string> = { viewer: "İzleyici", analyst: "Analist", admin: "Yönetici" };

function ApiStatus() {
  const [ok, setOk] = useState<boolean | null>(null);
  useEffect(() => {
    let alive = true;
    const check = () =>
      Api.health().then(() => alive && setOk(true)).catch(() => alive && setOk(false));
    check();
    const t = setInterval(check, 60_000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);
  return (
    <span className="inline-flex items-center gap-1.5 text-xs text-osiris-muted">
      <span className={`h-2 w-2 rounded-full ${ok === null ? "bg-osiris-faint" : ok ? "bg-osiris-ok" : "bg-osiris-bad"}`} />
      {ok === null ? "API denetleniyor" : ok ? "API çevrimiçi" : "API yanıt vermiyor"}
    </span>
  );
}

function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const { role, logout } = useAuth();
  const { draft } = useReportDraft();
  return (
    <div className="flex h-full flex-col">
      <div className="px-5 pb-5 pt-6">
        <div className="font-mono text-lg font-medium tracking-[0.35em] text-osiris-accent">OSIRIS</div>
        <div className="mt-1 text-xs text-osiris-faint">Açık kaynak istihbarat platformu</div>
      </div>
      <nav className="flex-1 space-y-5 overflow-y-auto px-3" aria-label="Ana menü">
        {NAV.map((g) => {
          const items = g.items.filter((i) => !i.min || i.min !== "admin" || can(role, "admin"));
          if (items.length === 0) return null;
          return (
            <div key={g.group}>
              <div className="px-2 pb-1.5 text-xs text-osiris-faint">{g.group}</div>
              <ul className="space-y-0.5">
                {items.map((i) => {
                  const locked = i.min && !can(role, i.min);
                  return (
                    <li key={i.to}>
                      <NavLink
                        to={i.to}
                        end={i.to === "/"}
                        onClick={onNavigate}
                        className={({ isActive }) =>
                          `flex items-center justify-between gap-2 rounded-md px-2.5 py-1.5 text-sm transition-colors ${
                            isActive
                              ? "bg-osiris-accent/10 text-osiris-ink shadow-[inset_2px_0_0_#22d3ee]"
                              : "text-osiris-muted hover:bg-osiris-panel hover:text-osiris-ink"
                          }`
                        }
                      >
                        <span>{i.label}</span>
                        {locked && <span className="text-[11px] text-osiris-faint">analist</span>}
                        {i.to === "/reports" && draft.findings.length > 0 && (
                          <span className="tnum rounded-full bg-osiris-accent/15 px-1.5 text-[11px] text-osiris-accent">
                            {draft.findings.length}
                          </span>
                        )}
                      </NavLink>
                    </li>
                  );
                })}
              </ul>
            </div>
          );
        })}
      </nav>
      <div className="space-y-2 border-t border-osiris-line px-5 py-4">
        {import.meta.env.VITE_DEMO === "1" && (
          <p className="rounded-md border border-osiris-warn/40 bg-osiris-warn/10 px-2.5 py-1.5 text-xs text-amber-100">
            Demo: örnek verilerle tarayıcıda çalışır, hiçbir sunucuya bağlanmaz.
          </p>
        )}
        <ApiStatus />
        <div className="flex items-center justify-between gap-2">
          <span className="text-sm">{role ? ROLE_LABEL[role] : "?"}</span>
          <button onClick={logout} className="text-sm text-osiris-muted hover:text-osiris-ink">Çıkış yap</button>
        </div>
      </div>
    </div>
  );
}

function Shell() {
  const { role } = useAuth();
  const [menu, setMenu] = useState(false);
  const loc = useLocation();
  useEffect(() => setMenu(false), [loc.pathname]);
  if (!role) return <Login />;
  return (
    <div className="min-h-screen lg:grid lg:grid-cols-[15rem_minmax(0,1fr)]">
      <aside className="sticky top-0 hidden h-screen border-r border-osiris-line bg-osiris-sink lg:block">
        <Sidebar />
      </aside>
      <div className="sticky top-0 z-30 flex items-center justify-between border-b border-osiris-line bg-osiris-sink/95 px-4 py-3 backdrop-blur lg:hidden">
        <span className="font-mono tracking-[0.35em] text-osiris-accent">OSIRIS</span>
        <button className="btn-ghost py-1" onClick={() => setMenu(true)} aria-expanded={menu}>Menü</button>
      </div>
      {menu && (
        <div className="fixed inset-0 z-40 lg:hidden">
          <button className="absolute inset-0 bg-black/60" onClick={() => setMenu(false)} aria-label="Menüyü kapat" />
          <aside className="relative h-full w-72 max-w-[85vw] border-r border-osiris-line bg-osiris-sink">
            <Sidebar onNavigate={() => setMenu(false)} />
          </aside>
        </div>
      )}
      <main className="mx-auto w-full min-w-0 max-w-6xl px-4 pb-16 pt-6 sm:px-6 lg:px-8 lg:pt-8">
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/username" element={<UsernameScan />} />
          <Route path="/collect" element={<Collect />} />
          <Route path="/search" element={<Search />} />
          <Route path="/items" element={<Items />} />
          <Route path="/entities" element={<Entities />} />
          <Route path="/graph" element={<Graph />} />
          <Route path="/sources" element={<Sources />} />
          <Route path="/alerts" element={<Alerts />} />
          <Route path="/reports" element={<Reports />} />
          <Route path="/admin" element={<Admin />} />
          <Route path="*" element={<p className="text-osiris-muted">Bu sayfa yok.</p>} />
        </Routes>
      </main>
    </div>
  );
}

export default function App() {
  return (
    <HashRouter>
      <UiProvider>
        <AuthProvider>
          <ReportDraftProvider>
            <Shell />
          </ReportDraftProvider>
        </AuthProvider>
      </UiProvider>
    </HashRouter>
  );
}
