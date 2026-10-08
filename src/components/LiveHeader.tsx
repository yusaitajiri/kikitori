import { Check, Pencil } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "../ipc/commands";
import { reportError } from "../lib/actions";
import { useTranscript } from "../store/transcript";
import { ProjectButton } from "./Projects";
import { SourceDropdown } from "./SourcePicker";
import { inputClass } from "./ui";

/** The recording's title; a click renames it (the folder keeps its name). */
function LiveTitle() {
  const { t } = useTranslation();
  const sessionId = useTranscript((s) => s.sessionId);
  const title = useTranscript((s) => s.title);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const rename = async () => {
    const next = draft.trim();
    setEditing(false);
    if (!sessionId || !next || next === title) return;
    try {
      await commands.renameSession(sessionId, next);
      useTranscript.getState().setTitle(next);
    } catch (e) {
      reportError(e, "saveFailed");
    }
  };

  if (editing) {
    return (
      <div className="flex min-w-0 flex-1 items-center gap-1.5">
        <input
          aria-label={t("newTitle")}
          className={`${inputClass} !h-9 min-w-0 flex-1 !text-[15px] font-bold`}
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
    );
  }
  return (
    <button
      type="button"
      title={t("rename")}
      onClick={() => {
        setDraft(title ?? "");
        setEditing(true);
      }}
      className="group -mx-1.5 flex min-w-0 flex-1 items-center gap-2 rounded-lg px-1.5 py-1 text-left transition-colors hover:bg-surface-2"
    >
      <h2 className="min-w-0 truncate text-[15px] leading-snug font-bold">{title}</h2>
      <Pencil size={13} className="shrink-0 text-muted opacity-0 transition-opacity group-hover:opacity-100" />
    </button>
  );
}

/**
 * The top of the recording view, kept in place above the transcript as it scrolls: its project's
 * colour (FR-64), the title (click to rename) and what is being recorded, a dropdown to switch
 * it (FR-17).
 */
export function LiveHeader() {
  const sessionId = useTranscript((s) => s.sessionId);
  const project = useTranscript((s) => s.project);

  return (
    <div className="mx-[18px] flex shrink-0 animate-fade-in items-center gap-3 border-b border-line py-2">
      <span className="-ml-1.5 flex min-w-0 flex-1 items-center gap-1">
        {sessionId && <ProjectButton sessionId={sessionId} project={project} bare />}
        <LiveTitle />
      </span>
      <SourceDropdown className="max-w-[55%] min-w-0 shrink-0" />
    </div>
  );
}
