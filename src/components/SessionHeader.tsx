import { Check, ChevronDown, Pencil } from "lucide-react";
import { Fragment, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { useModelName } from "../hooks/useModelName";
import { commands } from "../ipc/commands";
import { openFolder, reportError } from "../lib/actions";
import { dayOf, dayText, durationParts, timeOf } from "../lib/format";
import { useTranscriptStore } from "../store/transcript";
import { inputClass } from "./ui";

/** The last folders of a path, the way Explorer's address bar reads: `Kikitori › 2026-10-03_1513_Zoom`. */
const shortPath = (folder: string) => folder.split(/[\\/]/).filter(Boolean).slice(-2).join(" › ");

/**
 * The top of a finished session, scrolling with its transcript: the title (click to rename), when
 * it started, and its length, lines and screenshots in big numerals. 詳細 opens the rest, closed
 * at first: the model it was transcribed with and the folder it was saved to.
 */
export function SessionHeader() {
  const { t, i18n } = useTranslation();
  const store = useTranscriptStore();
  const sessionId = store((s) => s.sessionId);
  const title = store((s) => s.title);
  const startedAt = store((s) => s.startedAt);
  const durationMs = store((s) => s.durationMs);
  const folder = store((s) => s.folder);
  const modelId = store((s) => s.modelId);
  const gpu = store((s) => s.gpu);
  const items = store((s) => s.items);
  const model = useModelName(modelId);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [details, setDetails] = useState(false);
  const detailsId = useId();
  const lines = items.filter((i) => i.kind === "segment").length;
  const shots = items.filter((i) => i.kind === "screenshot").length;

  const rename = async () => {
    const next = draft.trim();
    setEditing(false);
    if (!sessionId || !next || next === title) return;
    try {
      await commands.renameSession(sessionId, next);
      store.getState().setTitle(next);
    } catch (e) {
      reportError(e, "saveFailed");
    }
  };

  const day = startedAt ? dayText(dayOf(startedAt), i18n.language) : "";
  const when = startedAt ? t("startedWhen", { when: i18n.language === "ja" ? `${day}${timeOf(startedAt)}` : `${day}, ${timeOf(startedAt)}` }) : null;
  const modelText = [model?.full, gpu === undefined ? null : gpu ? t("gpu") : t("cpu")].filter(Boolean).join(" · ");
  const unit = { h: t("unitH"), m: t("unitM"), s: t("unitS") };
  const big = (n: number) => <b className="num mr-0.5 text-[26px] leading-none font-light tracking-tight text-fg">{n}</b>;

  return (
    <div className="mx-[18px] animate-fade-up border-b border-line pt-3 pb-3.5">
      {editing ? (
        <div className="flex items-center gap-2">
          <input
            aria-label={t("newTitle")}
            className={`${inputClass} !h-9 min-w-0 flex-1 !text-[19px] font-bold`}
            value={draft}
            autoFocus
            onChange={(e) => setDraft(e.target.value)}
            onBlur={rename}
            onKeyDown={(e) => {
              if (e.key === "Enter") void rename();
              if (e.key === "Escape") setEditing(false);
            }}
          />
          <button type="button" aria-label={t("save")} className="grid size-9 shrink-0 place-items-center rounded-[10px] bg-accent text-accent-fg" onMouseDown={(e) => e.preventDefault()} onClick={rename}>
            <Check size={15} />
          </button>
        </div>
      ) : (
        <button
          type="button"
          title={t("rename")}
          onClick={() => {
            setDraft(title ?? "");
            setEditing(true);
          }}
          className="group -mx-1.5 flex max-w-full items-center gap-2 rounded-lg px-1.5 py-0.5 text-left transition-colors hover:bg-surface-2"
        >
          <h2 className="min-w-0 text-[19px] leading-snug font-bold break-words">{title}</h2>
          <Pencil size={14} className="shrink-0 text-muted opacity-0 transition-opacity group-hover:opacity-100" />
        </button>
      )}
      {when && <div className="mt-0.5 text-[12px] text-muted">{when}</div>}
      <div className="mt-2.5 flex flex-wrap items-baseline gap-x-[18px] gap-y-1 text-[12px] text-muted">
        {durationMs !== undefined && (
          <span>
            {durationParts(durationMs).map(([n, u]) => (
              <Fragment key={u}>
                {big(n)}
                <span className="mr-1">{unit[u]}</span>
              </Fragment>
            ))}
          </span>
        )}
        <span>
          {big(lines)}
          {t("unitLines")}
        </span>
        {shots > 0 && (
          <span>
            {big(shots)}
            {t("unitShots")}
          </span>
        )}
        {(modelText || folder) && (
          <button
            type="button"
            aria-expanded={details}
            aria-controls={detailsId}
            onClick={() => setDetails(!details)}
            className="-mr-1 ml-auto flex items-center gap-0.5 rounded-md px-1 font-semibold transition-colors hover:text-fg"
          >
            {t("details")}
            <ChevronDown size={13} className={`transition-transform duration-200 ${details ? "rotate-180" : ""}`} />
          </button>
        )}
      </div>
      {details && (
        <dl id={detailsId} className="mt-2.5 grid animate-fade-in grid-cols-[auto_minmax(0,1fr)] items-baseline gap-x-3 gap-y-1 text-[12px]">
          {modelText && (
            <>
              <dt className="text-muted">{t("model")}</dt>
              <dd className="min-w-0 truncate">{modelText}</dd>
            </>
          )}
          {folder && (
            <>
              <dt className="text-muted">{t("savedTo")}</dt>
              <dd className="flex min-w-0">
                <button type="button" title={folder} onClick={() => openFolder(folder)} className="min-w-0 truncate border-b border-line-strong text-left transition-colors hover:border-fg">
                  {shortPath(folder)}
                </button>
              </dd>
            </>
          )}
        </dl>
      )}
    </div>
  );
}
