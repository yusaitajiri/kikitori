import { create } from "zustand";
import { commands } from "../ipc/commands";
import type { Project } from "../ipc/types";

type ProjectsStore = {
  projects: Project[];
  /** Counts changes to which session is in which project, so lists of sessions reload. */
  version: number;
  load: () => Promise<void>;
  create: (name: string, color: string) => Promise<Project>;
  update: (project: Project) => Promise<void>;
  remove: (id: string) => Promise<void>;
  /** Puts sessions in a project, or (`null`) in none (FR-64). */
  assign: (sessionIds: string[], project: string | null) => Promise<void>;
};

export const useProjects = create<ProjectsStore>((set) => ({
  projects: [],
  version: 0,
  load: async () => set({ projects: await commands.listProjects() }),
  create: async (name, color) => {
    const project = await commands.createProject(name, color);
    set((s) => ({ projects: [...s.projects.filter((p) => p.id !== project.id), project] }));
    return project;
  },
  update: async (project) => set({ projects: await commands.updateProject(project) }),
  remove: async (id) => {
    const projects = await commands.deleteProject(id);
    // Its sessions now show as in no project.
    set((s) => ({ projects, version: s.version + 1 }));
  },
  assign: async (sessionIds, project) => {
    await commands.setProject(sessionIds, project);
    set((s) => ({ version: s.version + 1 }));
  },
}));

/** The project a session's ID names, if it still exists. */
export const projectOf = (projects: Project[], id?: string | null) => (id ? projects.find((p) => p.id === id) : undefined);
