// Projects (FR-64): their colours, and the menu that puts sessions in one.

import { useTranslation } from "react-i18next";
import type { Project } from "../ipc/types";
import { projectOf, useProjects } from "../store/projects";
import { useTranscriptStore } from "../store/transcript";
import { useUi } from "../store/ui";
import { reportError } from "./actions";
import { menuText, popupMenu, type MenuEntry } from "./popupMenu";

/** The colours a project can have (`session::projects::COLORS`); `index.css` shades each for light and dark. */
export const PROJECT_COLORS = ["slate", "blue", "teal", "green", "amber", "violet", "brown"] as const;

/** The i18n key naming a colour: `colorBlue`. */
export const colorKey = (c: string) => `color${c[0].toUpperCase()}${c.slice(1)}`;

/** A colour the other projects don't use yet, so a new one stands apart. */
export function freshColor(projects: Project[]): string {
  const used = new Set(projects.map((p) => p.color));
  return PROJECT_COLORS.find((c) => c !== "slate" && !used.has(c)) ?? "slate";
}

/**
 * Menu entries choosing a project: none, each project, and a new one. `pick` gets the project's ID
 * (or `null`); `newProject` asks for a name first.
 */
export function projectEntries(t: (k: string) => string, projects: Project[], current: string | undefined, pick: (id: string | null) => void, newProject: () => void): MenuEntry[] {
  const known = !!projectOf(projects, current);
  return [
    { text: t("noProject"), checked: !known, action: () => pick(null) },
    ...projects.map((p) => ({ text: menuText(p.name), checked: p.id === current, action: () => pick(p.id) })),
    "separator",
    { text: t("newProjectMenu"), action: newProject },
  ];
}

/** Opens the project menu for these sessions; the transcript on screen follows the choice. */
export function useProjectMenu() {
  const { t } = useTranslation();
  const store = useTranscriptStore();
  return async (sessionIds: string[], current: string | undefined) => {
    const { load, assign } = useProjects.getState();
    await load().catch(() => {});
    const done = (id: string | null) => {
      const shown = store.getState().sessionId;
      if (shown && sessionIds.includes(shown)) store.getState().setProject(id ?? undefined);
    };
    const pick = (id: string | null) =>
      void assign(sessionIds, id)
        .then(() => done(id))
        .catch((e) => reportError(e, "saveFailed"));
    const entries = projectEntries(t, useProjects.getState().projects, current, pick, () => useUi.getState().askNewProject(sessionIds, done));
    await popupMenu(entries).catch(reportError);
  };
}
