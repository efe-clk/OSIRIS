import { useState } from "react";
import { ApiError } from "../api";
import { useAuth } from "../auth";
import { Segmented } from "../ui";
import { errMsg } from "../util";

export default function Login() {
  const { login } = useAuth();
  const [mode, setMode] = useState<"key" | "jwt">("key");
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const v = value.trim();
      await login(mode === "key" ? { key: v } : { jwt: v });
    } catch (err) {
      setError(
        err instanceof ApiError && err.status === 401
          ? "Bu kimlik bilgisi geçerli değil. Kontrol edip yeniden deneyin."
          : errMsg(err, "Giriş başarısız"),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center p-4">
      <div className="w-full max-w-md">
        <div className="mb-8">
          <div className="font-mono text-2xl tracking-[0.4em] text-osiris-accent">OSIRIS</div>
          <p className="mt-2 text-sm text-osiris-muted">
            Open Source Intelligence Research &amp; Information System
          </p>
        </div>
        <form onSubmit={submit} className="panel space-y-4 p-6">
          <div className="flex items-center justify-between gap-3">
            <h1 className="text-lg font-semibold">Giriş yap</h1>
            <Segmented
              label="Kimlik türü"
              value={mode}
              onChange={(m) => { setMode(m); setError(null); }}
              options={[{ value: "key", label: "API anahtarı" }, { value: "jwt", label: "JWT" }]}
            />
          </div>
          <div>
            <label htmlFor="cred" className="mb-1 block text-sm text-osiris-muted">
              {mode === "key" ? "Operatör ya da kullanıcı anahtarı" : "Bearer jeton"}
            </label>
            <input
              id="cred"
              type="password"
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder={mode === "key" ? "osiris_…" : "eyJhbGciOi…"}
              autoComplete="off"
              autoFocus
              className="field font-mono"
            />
          </div>
          {error && <p role="alert" className="text-sm text-osiris-bad">{error}</p>}
          <button type="submit" disabled={busy || !value.trim()} className="btn-primary w-full">
            {busy ? "Doğrulanıyor" : "Giriş yap"}
          </button>
          <p className="text-xs leading-relaxed text-osiris-faint">
            Kimlik bilgisi yalnızca bu sekmede tutulur ve sekme kapanınca silinir. Rolünüz
            (izleyici, analist, yönetici) giriş sırasında sunucudan öğrenilir.
          </p>
        </form>
      </div>
    </div>
  );
}
