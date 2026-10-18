// OSIRIS: Swagger UI başlatıcı. Satır içi betik yerine ayrı dosya (Nginx CSP: script-src 'self').
// Belge göreli yoldan yüklenir: /api/docs → /api/openapi.json, doğrudan /docs → /openapi.json.
window.addEventListener("load", function () {
  window.ui = SwaggerUIBundle({ url: "openapi.json", dom_id: "#swagger-ui" });
});
