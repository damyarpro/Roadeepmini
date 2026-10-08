// English strings for the planner's coded errors (src-tauri/src/errors.rs,
// PLANNER_*). Registered through registerMessages by core/error-text.ts.

export const plannerErrorsEn: Record<string, string> = {
  "err.planner.store": "Couldn't read or save your planner on this PC: {detail}",
  "err.planner.invalid": "One of the values isn't valid — check it and try again.",
  "err.planner.notFound": "That item no longer exists.",
  "err.planner.limit": "The list is full ({limit} items). Delete a few first.",
};
