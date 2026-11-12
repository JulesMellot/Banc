// All visible interface text is in `fr.json`.
// A component never contains hard-coded text: it calls `t('key')`.
import fr from './fr.json';

export type MessageKey = keyof typeof fr;

/** Replaces each `{name}` in the template with the matching value. */
export function format(template: string, values: Record<string, string | number> = {}): string {
  return template.replace(/\{(\w+)\}/g, (placeholder, name: string) => {
    const value = values[name];
    return value === undefined ? placeholder : String(value);
  });
}

/** French text for the key, with its values. */
export function t(key: MessageKey, values?: Record<string, string | number>): string {
  return format(fr[key], values);
}
