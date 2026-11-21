/** Filters the live tool catalog for the sidebar and dashboard. */
import { tools } from "../../const/dashboard/tools";

// Match each word across the tool's name, description, and common search terms.
export function searchTools(query: string) {
  const terms = query.toLowerCase().trim().split(/\s+/);

  return tools.filter((tool) => {
    const text = `${tool.name} ${tool.description} ${tool.keywords}`.toLowerCase();
    return terms.every((term) => text.includes(term));
  });
}
