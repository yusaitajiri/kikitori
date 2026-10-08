import { Check, ChevronDown, Ellipsis, History as HistoryIcon, Search, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { SessionLine } from "../components/SessionLine";
import { Button, Dialog, IconButton, inputClass } from "../components/ui";
import { PageTitle } from "../components/PageTitle";
import { commands } from "../ipc/commands";
import type { SessionSummary } from "../ipc/types";
import { openFolder, reportError } from "../lib/actions";
import { dayName, dayOf, dayText, durationText, timeOf } from "../lib/format";
import { shapeFromActivity } from "../lib/line";
import { menuText, popupMenu, type MenuEntry } from "../lib/popupMenu";
import { ProjectMark } from "../components/Projects";
import { projectEntries } from "../lib/projects";
import { projectOf, useProjects } from "../store/projects";
import { useUi } from "../store/ui";

const norm = (s: string) => s.normalize("NFKC").toLowerCase();

/** Shorter than this, a recording is most likely a test or started by accident. */
const SHORT_MS = 10_000;

/** A recording under 10 s or with no transcript: greyed out (still readable, still openable), with no line. */
const isEmptySession = (s: Pick<SessionSummary, "segments" | "durationMs">) => s.segments === 0 || s.durationMs < SHORT_MS;

/** A row's checkbox in selection mode: a rounded square, ticked in ink. */
function Tick({ on }: { on: boolean }) {
  return (
    <span aria-hidden className={`grid size-[18px] shrink-0 place-items-center rounded-[5px] border-[1.5px] transition-colors duration-150 ${on ? "border-fg bg-fg text-bg" : "border-line-strong bg-surface"}`}>
      {on && <Check size={12} strokeWidth={3.2} />}
    </span>
  );
}

function Row({ s, onMenu, selecting, checked, onToggle }: { s: SessionSummary; onMenu: () => void; selecting: boolean; checked: boolean; onToggle: () => void }) {
  const { t } = useTranslation();
  const go = useUi((st) => st.go);
  const two = s.sources.includes("mic") && s.sources.some((id) => id !== "mic");
  const empty = isEmptySession(s);
  const shape = useMemo(() => (s.activity ? shapeFromActivity(s.activity) : null), [s.activity]);
  const project = useProjects((st) => projectOf(st.projects, s.project));
  return (
    <li className="group relative animate-fade-up">
      <button
        type="button"
        role={selecting ? "checkbox" : undefined}
        aria-checked={selecting ? checked : undefined}
        onClick={() => (selecting ? onToggle() : go("session", { sessionId: s.id }))}
        className={`flex w-full items-center gap-3 rounded-xl px-2 pt-2.5 pb-2 text-left transition-colors hover:bg-surface-2 ${checked ? "bg-surface-2" : ""} ${empty ? "text-muted" : ""}`}
      >
        {selecting && <Tick on={checked} />}
        <span className="grid min-w-0 flex-1 grid-cols-[minmax(0,1fr)_auto] gap-x-2.5">
          <span className="flex min-w-0 items-center gap-2">
            {project && (
              <span title={project.name} className="flex shrink-0">
                <ProjectMark color={project.color} />
                <span className="sr-only">{project.name}</span>
              </span>
            )}
            <span className={`min-w-0 truncate text-[13.5px] ${empty ? "font-medium" : "font-semibold"}`}>{s.title}</span>
            {s.recoverable && <span className="shrink-0 rounded-[5px] bg-alert px-1.5 text-[10.5px] font-bold text-alert-fg">{t("needsRecovery")}</span>}
          </span>
          <span className={`num text-[11.5px] text-muted transition-opacity ${selecting ? "" : "group-focus-within:opacity-0 group-hover:opacity-0"}`}>{timeOf(s.startedAt)}</span>
          {shape && !empty && <SessionLine shape={shape} two={two} className="col-span-2 mt-1.5 mb-1" />}
          <span className="min-w-0 truncate text-[12px] text-muted">{s.preview || t("emptyTranscript")}</span>
          <span className="text-[11.5px] text-muted">{durationText(s.durationMs, t)}</span>
        </span>
      </button>
      {!selecting && (
        <IconButton
          size="sm"
          label={t("moreActions")}
          aria-haspopup="menu"
          onClick={onMenu}
          className="absolute top-1.5 right-1.5 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
        >
          <Ellipsis size={16} />
        </IconButton>
      )}
    </li>
  );
}

/** Which sessions the list shows: all, one project's, or those in none. */
type ProjectFilter = { kind: "all" } | { kind: "none" } | { kind: "project"; id: string };

/**
 * Past sessions by day, each with its own line; searchable by title and opening words (FR-62,
 * FR-63), and narrowed to one project (FR-64). 選択 picks several to move to the Recycle Bin at
 * once, such as all the short and empty ones, or to put in a project.
 */
export function History() {
  const { t, i18n } = useTranslation();
  const setExportFor = useUi((s) => s.setExportFor);
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [query, setQuery] = useState("");
  const [deleting, setDeleting] = useState<SessionSummary | null>(null);
  const [renaming, setRenaming] = useState<SessionSummary | null>(null);
  const [newTitle, setNewTitle] = useState("");
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set());
  const [deletingMany, setDeletingMany] = useState(false);
  const [now] = useState(() => new Date());
  const [chosenFilter, setFilter] = useState<ProjectFilter>({ kind: "all" });
  const projects = useProjects((s) => s.projects);
  const askNewProject = useUi((s) => s.askNewProject);
  // A deleted project can't stay the filter.
  const filter = useMemo<ProjectFilter>(
    () => (chosenFilter.kind === "project" && !projectOf(projects, chosenFilter.id) ? { kind: "all" } : chosenFilter),
    [chosenFilter, projects],
  );

  const refresh = async () => {
    try {
      setSessions(await commands.listSessions());
    } catch (e) {
      reportError(e);
      setSessions([]);
    }
  };

  useEffect(() => {
    void useProjects.getState().load().catch(() => {});
  }, []);

  // A project changed somewhere (a menu here, the new-project dialog, 設定): reload the rows.
  useEffect(
    () =>
      useProjects.subscribe((s, prev) => {
        if (s.version !== prev.version) void refresh();
      }),
    [], // eslint-disable-line react-hooks/exhaustive-deps
  );

  useEffect(() => {
    let alive = true;
    commands
      .listSessions()
      .then((s) => alive && setSessions(s))
      .catch((e) => {
        reportError(e);
        if (alive) setSessions([]);
      });
    return () => {
      alive = false;
    };
  }, []);

  const days = useMemo(() => {
    const q = norm(query.trim());
    const inFilter = (s: SessionSummary) => {
      const known = projectOf(projects, s.project);
      if (filter.kind === "none") return !known;
      if (filter.kind === "project") return known?.id === filter.id;
      return true;
    };
    const found = (sessions ?? []).filter((s) => inFilter(s) && (!q || norm(s.title).includes(q) || norm(s.preview).includes(q)));
    const out: { day: string; items: SessionSummary[] }[] = [];
    for (const s of found) {
      const day = dayOf(s.startedAt);
      const last = out[out.length - 1];
      if (last?.day === day) last.items.push(s);
      else out.push({ day, items: [s] });
    }
    return out;
  }, [sessions, query, filter, projects]);

  // Only what the search shows can be selected, so nothing hidden is deleted.
  const visible = useMemo(() => days.flatMap((d) => d.items), [days]);
  const chosen = visible.filter((s) => selected.has(s.id));
  const empties = visible.filter(isEmptySession);
  const allChosen = visible.length > 0 && chosen.length === visible.length;
  const choose = (list: SessionSummary[]) => setSelected(new Set(list.map((s) => s.id)));
  const toggle = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const stopSelecting = () => {
    setSelecting(false);
    setSelected(new Set());
  };

  // Escape leaves selection mode (in the search field it clears the search instead).
  useEffect(() => {
    if (!selecting || deletingMany) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || (e.target as HTMLElement).tagName === "INPUT") return;
      setSelecting(false);
      setSelected(new Set());
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selecting, deletingMany]);

  const deleteChosen = async () => {
    const ids = chosen.map((s) => s.id);
    setDeletingMany(false);
    try {
      const r = await commands.deleteSessions(ids);
      useUi.getState().toast(t("deletedMany", { n: r.deleted }));
    } catch (e) {
      reportError(e, "deleteFailed");
    }
    stopSelecting();
    void refresh();
  };

  const run = async (f: () => Promise<unknown>, okKey?: string) => {
    try {
      await f();
      if (okKey) useUi.getState().toast(t(okKey));
    } catch (e) {
      reportError(e);
    }
    void refresh();
  };

  const assign = (ids: string[], project: string | null) =>
    void useProjects
      .getState()
      .assign(ids, project)
      .catch((e) => reportError(e, "saveFailed"));
  const projectMenu = (ids: string[], current?: string) => projectEntries(t, projects, current, (id) => assign(ids, id), () => askNewProject(ids));

  const filterMenu = async () => {
    const entries: MenuEntry[] = [
      { text: t("allProjects"), checked: filter.kind === "all", action: () => setFilter({ kind: "all" }) },
      ...projects.map((p) => ({ text: menuText(p.name), checked: filter.kind === "project" && filter.id === p.id, action: () => setFilter({ kind: "project", id: p.id }) })),
      { text: t("noProject"), checked: filter.kind === "none", action: () => setFilter({ kind: "none" }) },
    ];
    await popupMenu(entries).catch(reportError);
  };
  const filtered = filter.kind === "project" ? projectOf(projects, filter.id) : undefined;

  const menu = async (s: SessionSummary) => {
    const entries: MenuEntry[] = [
      ...(s.recoverable ? ([{ text: t("recover"), action: () => void run(() => commands.recoverSession(s.id), "recovered") }, "separator"] satisfies MenuEntry[]) : []),
      {
        text: t("rename"),
        action: () => {
          setNewTitle(s.title);
          setRenaming(s);
        },
      },
      { text: t("project"), items: projectMenu([s.id], s.project) },
      { text: t("exportDialog"), action: () => setExportFor(s.id) },
      { text: t("openFolder"), action: () => void openFolder(s.folder) },
      "separator",
      { text: t("delete"), action: () => setDeleting(s) },
    ];
    await popupMenu(entries).catch(reportError);
  };

  const dayLabel = (day: string) => {
    const name = dayName(day, now);
    return { name: name === "today" ? t("today") : name === "yesterday" ? t("yesterday") : null, date: dayText(day, i18n.language, now.getFullYear()) };
  };

  return (
    <div className="flex h-full flex-col">
      <PageTitle
        title={t("historyTitle")}
        sub={sessions && sessions.length > 0 ? t("historyCount", { n: sessions.length }) : undefined}
        action={
          sessions && sessions.length > 0 ? (
            <Button variant="ghost" size="sm" onClick={() => (selecting ? stopSelecting() : setSelecting(true))} aria-pressed={selecting}>
              {selecting ? t("done") : t("select")}
            </Button>
          ) : undefined
        }
      >
        <div className="mt-2.5 flex gap-1.5">
          <label className="group relative block min-w-0 flex-1">
            <Search size={15} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-muted transition-colors group-focus-within:text-fg" />
            <input
              type="search"
              aria-label={t("searchHistory")}
              placeholder={t("searchHistory")}
              className={`${inputClass} !h-[38px] !rounded-[11px] pr-8 pl-9 placeholder:text-muted [&::-webkit-search-cancel-button]:hidden`}
              value={query}
              autoFocus
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && setQuery("")}
            />
            {query && (
              <button type="button" aria-label={t("close")} onClick={() => setQuery("")} className="absolute top-1/2 right-2 grid size-5 -translate-y-1/2 place-items-center rounded-full text-muted hover:bg-surface-2 hover:text-fg">
                <X size={12} />
              </button>
            )}
          </label>
          {projects.length > 0 && (
            <button
              type="button"
              aria-haspopup="menu"
              aria-label={`${t("project")}: ${filtered?.name ?? (filter.kind === "none" ? t("noProject") : t("allProjects"))}`}
              onClick={filterMenu}
              className={`flex h-[38px] max-w-[140px] shrink-0 items-center gap-1.5 rounded-[11px] border px-2.5 text-[12.5px] transition-colors hover:border-line-strong ${filter.kind === "all" ? "border-line text-muted" : "border-fg font-semibold"}`}
            >
              {filter.kind !== "all" && <ProjectMark color={filtered?.color} />}
              <span className="min-w-0 truncate">{filtered?.name ?? (filter.kind === "none" ? t("noProject") : t("project"))}</span>
              <ChevronDown size={13} className="shrink-0 text-muted" />
            </button>
          )}
        </div>
        {selecting && (
          <div className="mt-2 flex animate-fade-in flex-wrap items-center gap-x-4 gap-y-1 px-0.5">
            <button type="button" className="kk-link !px-0" onClick={() => choose(allChosen ? [] : visible)}>
              {allChosen ? t("selectNone") : t("selectAll")}
            </button>
            {empties.length > 0 && (
              <button type="button" className="kk-link !px-0" onClick={() => choose(empties)}>
                {t("selectEmpty", { n: empties.length })}
              </button>
            )}
          </div>
        )}
      </PageTitle>
      <div className="min-h-0 flex-1 overflow-y-auto border-t border-line px-2.5 pb-3">
        {sessions === null ? (
          <div className="space-y-2 px-1 pt-3" aria-label={t("loading")}>
            {[0, 1, 2].map((i) => (
              <div key={i} className="h-[86px] animate-pulse rounded-xl bg-surface-2" style={{ animationDelay: `${i * 120}ms` }} />
            ))}
          </div>
        ) : sessions.length === 0 ? (
          <div className="flex h-full animate-fade-in flex-col items-center justify-center gap-3 pb-10 text-center text-muted">
            <HistoryIcon size={26} />
            {t("historyEmpty")}
          </div>
        ) : days.length === 0 ? (
          <div className="animate-fade-in p-8 text-center text-muted">{t("historyNoMatch")}</div>
        ) : (
          days.map(({ day, items }) => {
            const label = dayLabel(day);
            return (
              <section key={day}>
                <h3 className="sticky top-0 z-10 flex items-baseline gap-2 bg-bg px-2 pt-3.5 pb-1 text-[12px]">
                  {label.name && <b className="font-bold">{label.name}</b>}
                  <span className={label.name ? "text-muted" : "font-bold"}>{label.date}</span>
                </h3>
                <ul className="space-y-0.5">
                  {items.map((s) => (
                    <Row key={s.id} s={s} onMenu={() => menu(s)} selecting={selecting} checked={selected.has(s.id)} onToggle={() => toggle(s.id)} />
                  ))}
                </ul>
              </section>
            );
          })
        )}
      </div>
      {selecting && (
        <div className="flex shrink-0 animate-fade-in items-center justify-between gap-3 border-t border-line px-[18px] py-2.5">
          <span className="text-[12.5px] text-muted" aria-live="polite">
            {t("selectedCount", { n: chosen.length })}
          </span>
          <span className="flex gap-2">
            <Button
              size="lg"
              disabled={chosen.length === 0}
              aria-haspopup="menu"
              onClick={() => void popupMenu(projectMenu(chosen.map((s) => s.id))).catch(reportError)}
            >
              {t("project")}
            </Button>
            <Button variant="primary" size="lg" disabled={chosen.length === 0} onClick={() => setDeletingMany(true)}>
              {t("delete")}
            </Button>
          </span>
        </div>
      )}
      {deletingMany && (
        <Dialog
          title={t("confirmDeleteMany", { n: chosen.length })}
          onClose={() => setDeletingMany(false)}
          actions={
            <>
              <Button onClick={() => setDeletingMany(false)}>{t("cancel")}</Button>
              <Button variant="danger" onClick={() => void deleteChosen()}>
                {t("delete")}
              </Button>
            </>
          }
        />
      )}
      {deleting && (
        <Dialog
          title={t("confirmDelete", { title: deleting.title })}
          onClose={() => setDeleting(null)}
          actions={
            <>
              <Button onClick={() => setDeleting(null)}>{t("cancel")}</Button>
              <Button
                variant="danger"
                onClick={() => {
                  const id = deleting.id;
                  setDeleting(null);
                  void run(() => commands.deleteSession(id), "deleted");
                }}
              >
                {t("delete")}
              </Button>
            </>
          }
        />
      )}
      {renaming && (
        <Dialog
          title={t("rename")}
          onClose={() => setRenaming(null)}
          actions={
            <>
              <Button onClick={() => setRenaming(null)}>{t("cancel")}</Button>
              <Button
                variant="primary"
                disabled={!newTitle.trim()}
                onClick={() => {
                  const id = renaming.id;
                  setRenaming(null);
                  void run(() => commands.renameSession(id, newTitle.trim()), "saved");
                }}
              >
                {t("save")}
              </Button>
            </>
          }
        >
          <input
            aria-label={t("newTitle")}
            className={inputClass}
            value={newTitle}
            autoFocus
            onChange={(e) => setNewTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && newTitle.trim()) {
                const id = renaming.id;
                setRenaming(null);
                void run(() => commands.renameSession(id, newTitle.trim()), "saved");
              }
            }}
          />
        </Dialog>
      )}
    </div>
  );
}
