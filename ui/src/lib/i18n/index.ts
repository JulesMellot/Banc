// All visible interface text is in `fr.json`.
// A component never contains hard-coded text: it calls `t('cle')`.
import fr from './fr.json';

export type CleMessage = keyof typeof fr;

/** Replaces each `{nom}` in the template with the matching value. */
export function formater(modele: string, valeurs: Record<string, string | number> = {}): string {
  return modele.replace(/\{(\w+)\}/g, (marque, nom: string) => {
    const valeur = valeurs[nom];
    return valeur === undefined ? marque : String(valeur);
  });
}

/** French text for the key, with its values. */
export function t(cle: CleMessage, valeurs?: Record<string, string | number>): string {
  return formater(fr[cle], valeurs);
}
