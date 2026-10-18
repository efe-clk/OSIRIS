import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Api, can, type ItemDetail } from "./api";
import { useAuth } from "./auth";
import { useReportDraft } from "./reportDraft";
import { Drawer, EntityTypeTag, KeyValue, Loading, Notice, useUi } from "./ui";
import { errMsg, fmtDate, safeHref } from "./util";

/** Öğe detayı (GET /items/{id}): içerik, varlıklar, rapora ekleme, KVKK silme. */
export function ItemDrawer({ id, onClose, onDeleted }: { id: string | null; onClose: () => void; onDeleted?: (id: string) => void }) {
  const { role } = useAuth();
  const { confirm, toast } = useUi();
  const { addFinding, addEntities } = useReportDraft();
  const nav = useNavigate();
  const [item, setItem] = useState<ItemDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    setItem(null);
    setError(null);
    Api.item(id).then(setItem).catch((e) => setError(errMsg(e)));
  }, [id]);

  const remove = async () => {
    if (!item) return;
    const ok = await confirm({
      title: "Öğe kalıcı olarak silinsin mi?",
      body: "Öğe ve yalnızca ona bağlı varlıklar veritabanından silinir. Denetim kaydına yalnızca öğe kimliği yazılır. Bu işlem geri alınamaz.",
      confirmLabel: "Öğeyi sil",
      danger: true,
    });
    if (!ok) return;
    try {
      const r = await Api.deleteItem(item.id);
      toast(`Öğe silindi (${r.entities_deleted} bağsız varlık da silindi)`);
      onDeleted?.(item.id);
      onClose();
    } catch (e) {
      toast(errMsg(e), "error");
    }
  };

  const href = safeHref(item?.url);

  return (
    <Drawer open={!!id} onClose={onClose} wide title={item?.title || (item ? "Başlıksız öğe" : "Öğe")}>
      {error && <Notice>{error}</Notice>}
      {!item && !error && <Loading />}
      {item && (
        <div className="space-y-6">
          <div className="flex flex-wrap gap-2">
            <button
              className="btn-ghost"
              onClick={() => {
                addFinding(
                  { title: item.title || item.url || "Öğe", description: (item.cleaned_content || "").slice(0, 600) },
                  item.url,
                );
                toast("Bulgu rapor taslağına eklendi");
              }}
            >
              Rapora ekle
            </button>
            {item.entities.length > 0 && (
              <button
                className="btn-ghost"
                onClick={() => {
                  addEntities(item.entities.map((e) => ({ type: e.type, value: e.value })));
                  toast(`${item.entities.length} varlık rapora gösterge olarak eklendi`);
                }}
              >
                Varlıkları göstergelere ekle
              </button>
            )}
            {href && (
              <a className="btn-ghost" href={href} target="_blank" rel="noopener noreferrer">Kaynağı aç</a>
            )}
            {can(role, "admin") && <button className="btn-danger" onClick={remove}>Öğeyi sil</button>}
          </div>

          <KeyValue
            rows={[
              ["URL", href ? <a className="text-osiris-accent hover:underline" href={href} target="_blank" rel="noopener noreferrer">{item.url}</a> : item.url || "—"],
              ["Toplandı", fmtDate(item.collected_at)],
              ["Yayımlandı", fmtDate(item.published_at)],
              ["Dil", item.language || "—"],
              ["Etiketler", item.tags?.length ? item.tags.join(", ") : "—"],
              ["Kaynak", item.source_id ? <span className="font-mono text-xs">{item.source_id}</span> : "Kaynaksız (ad hoc toplama)"],
              ["content_hash", <span className="break-all font-mono text-xs text-osiris-muted">{item.content_hash}</span>],
            ]}
          />

          <section>
            <h3 className="mb-2 text-sm font-semibold">Varlıklar ({item.entities.length})</h3>
            {item.entities.length === 0 ? (
              <p className="text-sm text-osiris-muted">Bu öğede varlık bulunmadı.</p>
            ) : (
              <ul className="flex flex-wrap gap-2">
                {item.entities.map((e) => (
                  <li key={`${e.type}:${e.value}`}>
                    <button
                      onClick={() => {
                        onClose();
                        nav(`/search?mode=entity&type=${e.type}&q=${encodeURIComponent(e.value)}`);
                      }}
                      title="Bu varlığı geçen tüm öğeleri ara"
                      className="flex max-w-full items-center gap-2 rounded-md border border-osiris-line bg-osiris-panel px-2 py-1 text-sm hover:border-osiris-faint"
                    >
                      <EntityTypeTag type={e.type} />
                      <span className="truncate font-mono text-[13px]">{e.value}</span>
                      {e.mention_count > 1 && <span className="tnum text-xs text-osiris-faint">×{e.mention_count}</span>}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section>
            <h3 className="mb-2 text-sm font-semibold">Temizlenmiş içerik</h3>
            <div className="max-h-[50vh] overflow-y-auto whitespace-pre-wrap rounded-md border border-osiris-line bg-osiris-sink p-4 text-sm leading-relaxed text-osiris-ink/90">
              {item.cleaned_content || "İçerik yok."}
            </div>
          </section>

          {item.metadata && Object.keys(item.metadata).length > 0 && (
            <section>
              <h3 className="mb-2 text-sm font-semibold">Üst veri</h3>
              <pre className="overflow-x-auto rounded-md border border-osiris-line bg-osiris-sink p-3 font-mono text-xs text-osiris-muted">
                {JSON.stringify(item.metadata, null, 2)}
              </pre>
            </section>
          )}
        </div>
      )}
    </Drawer>
  );
}
