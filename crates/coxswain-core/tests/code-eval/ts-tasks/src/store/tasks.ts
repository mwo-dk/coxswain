import { create } from "zustand";
import { request } from "../api/client";
import { Task, TaskList } from "../schema";

interface TasksState {
  tasks: Task[];
  loading: boolean;
  fetchTasks: () => Promise<void>;
  toggle: (id: string) => Promise<void>;
}

/** The tasks of the signed-in user, shared by every component that needs them. */
export const useTasks = create<TasksState>((set, get) => ({
  tasks: [],
  loading: false,
  fetchTasks: async () => {
    set({ loading: true });
    const tasks = TaskList.parse(await request<unknown>("/tasks"));
    set({ tasks, loading: false });
  },
  toggle: async (id) => {
    const task = get().tasks.find((t) => t.id === id);
    if (!task) return;
    const updated = Task.parse(await request<unknown>(`/tasks/${id}`, { method: "PATCH", body: JSON.stringify({ done: !task.done }) }));
    set({ tasks: get().tasks.map((t) => (t.id === id ? updated : t)) });
  },
}));
