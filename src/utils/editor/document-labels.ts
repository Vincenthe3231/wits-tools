/** Formats saved document titles consistently across the Editor library. */

// Keep the complete filename for actions and tooltips while giving titles room in rows.
export const documentTitle = (name: string) => name.replace(/\.(?:md|markdown|witsdoc)$/i, "") || name;
export const documentIcon = (name: string) => /\.witsdoc$/i.test(name) ? "visual-document" : "document";
