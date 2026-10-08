import { Check, ChevronDown, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Project } from "../ipc/types";
import { reportError } from "../lib/actions";
import { colorKey, freshColor, PROJECT_COLORS, useProjectMenu } from "../lib/projects";
import { projectOf, useProjects } from "../store/projects";
import { useUi } from "../store/ui";
import { Button, Dialog, inputClass } from "./ui";

/** A project's colour as a small rounded square (the dot is the app's only circle); hollow for none. */
export function ProjectMark({ color, size = 10, className = "" }: { color?: string; size?: number; className?: string }) {
  return (
    <span
      aria-hidden
      className={`inline-block shrink-0 rounded-[3px] ${color ? "" : "border-[1.5px] border-line-strong"} ${className}`}
      style={{ width: size, height: size, ...(color ? { background: `var(--kk-p-${color}, var(--kk-p-slate))` } : {}) }}
    />
  );
}

/** The colour choice: one square per colour, the chosen one ringed. */
export function ColorChoice({ value, onChange }: { value: string; onChange: (c: string) => void }) {
  const { t } = useTranslation();
  return (
    <div role="radiogroup" aria-label={t("projectColor")} className="flex flex-wrap gap-1.5">
      {PROJECT_COLORS.map((c) => (
        <button
          key={c}
          type="button"
          role="radio"
          aria-checked={value === c}
          aria-label={t(colorKey(c))}
          title={t(colorKey(c))}
          onClick={() => onChange(c)}
          className={`grid size-7 place-items-center rounded-[8px] border transition-colors ${value === c ? "border-fg" : "border-transparent hover:border-line-strong"}`}
        >
          <ProjectMark color={c} size={16} className="!rounded-[4px]" />
        </button>
      ))}
    </div>
  );
}

/** Names a new project and puts the sessions it was asked for in it. */
export function NewProjectDialog() {
  const ask = useUi((s) => s.newProject);
  // A fresh form for each ask.
  return ask ? <NewProjectForm key={ask.n} ask={ask} /> : null;
}

function NewProjectForm({ ask }: { ask: { sessionIds: string[]; done?: (id: string) => void } }) {
  const { t } = useTranslation();
  const close = () => useUi.getState().askNewProject(undefined);
  const [name, setName] = useState("");
  const [color, setColor] = useState(() => freshColor(useProjects.getState().projects));

  const create = async () => {
    if (!name.trim()) return;
    close();
    try {
      const { create, assign } = useProjects.getState();
      const project = await create(name.trim(), color);
      if (ask.sessionIds.length) {
        await assign(ask.sessionIds, project.id);
        ask.done?.(project.id);
      }
    } catch (e) {
      reportError(e, "saveFailed");
    }
  };
  return (
    <Dialog
      title={t("newProject")}
      onClose={close}
      actions={
        <>
          <Button onClick={close}>{t("cancel")}</Button>
          <Button variant="primary" disabled={!name.trim()} onClick={create}>
            {t("createProject")}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <input
          aria-label={t("projectName")}
          placeholder={t("projectName")}
          className={inputClass}
          value={name}
          maxLength={40}
          autoFocus
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void create()}
        />
        <ColorChoice value={color} onChange={setColor} />
      </div>
    </Dialog>
  );
}

/**
 * A session's project in its header: the colour and the name (or a hollow square and 「プロジェクトなし」),
 * opening the project menu. `bare` shows the square alone (the recording's header, beside its title).
 */
export function ProjectButton({ sessionId, project, bare = false }: { sessionId: string; project?: string; bare?: boolean }) {
  const { t } = useTranslation();
  const projects = useProjects((s) => s.projects);
  const menu = useProjectMenu();
  const p = projectOf(projects, project);
  const label = p ? `${t("project")}: ${p.name}` : t("noProject");

  useEffect(() => {
    void useProjects.getState().load().catch(() => {});
  }, []);

  if (bare) {
    return (
      <button type="button" aria-haspopup="menu" aria-label={label} title={label} onClick={() => menu([sessionId], project)} className="grid size-7 shrink-0 place-items-center rounded-lg transition-colors hover:bg-surface-2">
        <ProjectMark color={p?.color} size={11} />
      </button>
    );
  }
  return (
    <button type="button" aria-haspopup="menu" aria-label={label} onClick={() => menu([sessionId], project)} className="-mx-1 inline-flex max-w-full items-center gap-1.5 rounded-md px-1 py-0.5 text-[12px] transition-colors hover:bg-surface-2">
      <ProjectMark color={p?.color} />
      <span className={`min-w-0 truncate ${p ? "font-semibold text-fg" : "text-muted"}`}>{p ? p.name : t("noProject")}</span>
      <ChevronDown size={12} className="shrink-0 text-muted" />
    </button>
  );
}

/** 設定 › プロジェクト: rename, recolour or delete each project, or add one. */
export function ProjectsSettings() {
  const { t } = useTranslation();
  const { projects, load, update, remove } = useProjects();
  const [open, setOpen] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [deleting, setDeleting] = useState<Project | null>(null);

  useEffect(() => {
    void load().catch((e) => reportError(e));
  }, [load]);

  const save = (p: Project) => update(p).catch((e) => reportError(e, "saveFailed"));

  return (
    <>
      <section className="rounded-2xl border border-line bg-surface px-1.5 py-1">
        {projects.length === 0 && <p className="px-2 py-3 text-center text-[12.5px] text-muted">{t("projectsEmpty")}</p>}
        <ul className="divide-y divide-line">
          {projects.map((p) => (
            <li key={p.id}>
              <button
                type="button"
                aria-expanded={open === p.id}
                onClick={() => {
                  setName(p.name);
                  setOpen(open === p.id ? null : p.id);
                }}
                className="flex w-full items-center gap-2.5 rounded-xl px-2 py-2.5 text-left transition-colors hover:bg-surface-2"
              >
                <ProjectMark color={p.color} size={12} />
                <span className="min-w-0 flex-1 truncate text-[13.5px] font-semibold">{p.name}</span>
                <ChevronDown size={14} className={`shrink-0 text-muted transition-transform duration-200 ${open === p.id ? "rotate-180" : ""}`} />
              </button>
              {open === p.id && (
                <div className="animate-fade-in space-y-2.5 px-2 pt-1 pb-3">
                  <div className="flex gap-1.5">
                    <input
                      aria-label={t("projectName")}
                      className={`${inputClass} min-w-0 flex-1`}
                      value={name}
                      maxLength={40}
                      onChange={(e) => setName(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && name.trim() && void save({ ...p, name })}
                    />
                    <Button variant="primary" size="sm" className="!h-[38px]" disabled={!name.trim() || name.trim() === p.name} onClick={() => save({ ...p, name })} aria-label={t("save")}>
                      <Check size={14} />
                    </Button>
                  </div>
                  <ColorChoice value={p.color} onChange={(color) => save({ ...p, color })} />
                  <button type="button" className="kk-link !px-0 text-[12px]" onClick={() => setDeleting(p)}>
                    {t("deleteProject")}
                  </button>
                </div>
              )}
            </li>
          ))}
        </ul>
      </section>
      <Button variant="ghost" onClick={() => useUi.getState().askNewProject([])}>
        <Plus size={14} />
        {t("newProject")}
      </Button>
      {deleting && (
        <Dialog
          title={t("confirmDeleteProject", { name: deleting.name })}
          onClose={() => setDeleting(null)}
          actions={
            <>
              <Button onClick={() => setDeleting(null)}>{t("cancel")}</Button>
              <Button
                variant="danger"
                onClick={() => {
                  const id = deleting.id;
                  setDeleting(null);
                  setOpen(null);
                  void remove(id).catch((e) => reportError(e, "deleteFailed"));
                }}
              >
                {t("delete")}
              </Button>
            </>
          }
        >
          {t("deleteProjectNote")}
        </Dialog>
      )}
    </>
  );
}
