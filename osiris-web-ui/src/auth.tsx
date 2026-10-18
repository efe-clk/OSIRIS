import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { resolveRole, session, setUnauthorizedHandler, type Role } from "./api";

interface AuthState {
  role: Role | null;
  login: (cred: { key?: string; jwt?: string }) => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthState>({
  role: null,
  login: async () => undefined,
  logout: () => undefined,
});

export function AuthProvider({ children }: { children: ReactNode }) {
  const [role, setRole] = useState<Role | null>(() =>
    session.key || session.jwt ? session.role : null,
  );

  useEffect(() => setUnauthorizedHandler(() => setRole(null)), []);

  const login = useCallback(async (cred: { key?: string; jwt?: string }) => {
    const resolved = await resolveRole(cred);
    // Doğrulama bitmeden saklanmaz (yarım oturum kalmasın)
    session.save(cred, resolved);
    setRole(resolved);
  }, []);

  const logout = useCallback(() => {
    session.clear();
    setRole(null);
  }, []);

  return <AuthContext.Provider value={{ role, login, logout }}>{children}</AuthContext.Provider>;
}

export const useAuth = () => useContext(AuthContext);
