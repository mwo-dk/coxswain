import { z } from "zod";

/** A task as the API sends it, checked before it reaches the store. */
export const Task = z.object({
  id: z.string().uuid(),
  title: z.string().min(1).max(200),
  done: z.boolean(),
  due: z.string().datetime().nullable(),
  tags: z.array(z.string()).default([]),
});

export type Task = z.infer<typeof Task>;

export const TaskList = z.array(Task);
