import { useEffect, useState } from "react";
import { useTasks } from "../store/tasks";
import { debounce } from "../utils/debounce";

/** The list of tasks, with a filter box; loads the tasks when it is first shown. */
export function TaskList() {
  const { tasks, loading, fetchTasks, toggle } = useTasks();
  const [filter, setFilter] = useState("");
  const onFilter = debounce((value: string) => setFilter(value), 250);

  useEffect(() => {
    fetchTasks();
  }, [fetchTasks]);

  if (loading) return <p>Loading…</p>;
  const shown = tasks.filter((t) => t.title.toLowerCase().includes(filter.toLowerCase()));
  return (
    <section>
      <input placeholder="Filter" onChange={(e) => onFilter(e.target.value)} />
      <ul>
        {shown.map((t) => (
          <li key={t.id}>
            <label>
              <input type="checkbox" checked={t.done} onChange={() => toggle(t.id)} /> {t.title}
            </label>
          </li>
        ))}
      </ul>
    </section>
  );
}
